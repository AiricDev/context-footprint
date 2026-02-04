//! SCIP adapter - Converts SCIP index to new SemanticData format

use crate::adapters::scip::parser::parse_range;
use crate::domain::ports::SemanticDataSource;
use crate::domain::semantic::{
    DocumentSemantics, FunctionDetails, Mutability, ParameterInfo, ReferenceRole, SemanticData,
    SourceLocation, SourceSpan, SymbolDefinition, SymbolDetails, SymbolId, SymbolKind,
    SymbolReference, TypeDetails, TypeKind, VariableDetails, VariableKind, Visibility,
};
use crate::scip;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// SCIP data source adapter
pub struct ScipDataSourceAdapter {
    pub scip_path: std::path::PathBuf,
}

impl ScipDataSourceAdapter {
    pub fn new<P: AsRef<std::path::Path>>(path: P) -> Self {
        Self {
            scip_path: path.as_ref().to_path_buf(),
        }
    }
}

impl SemanticDataSource for ScipDataSourceAdapter {
    fn load(&self) -> Result<SemanticData> {
        // Load SCIP index
        let index = load_scip_index(&self.scip_path)?;

        // Create a global symbol lookup map
        let mut symbol_map: HashMap<SymbolId, scip::SymbolInformation> = HashMap::new();
        for sym in &index.external_symbols {
            symbol_map.insert(sym.symbol.clone(), sym.clone());
        }
        for doc in &index.documents {
            for sym in &doc.symbols {
                symbol_map.insert(sym.symbol.clone(), sym.clone());
            }
        }

        // Process each document
        let mut documents: Vec<DocumentSemantics> = Vec::new();

        for doc in &index.documents {
            let (definitions, references) =
                process_document(doc, &symbol_map, &index.external_symbols)?;

            documents.push(DocumentSemantics {
                relative_path: doc.relative_path.clone(),
                language: doc.language.clone(),
                definitions,
                references,
            });
        }

        // Process external symbols
        let external_symbols: Vec<SymbolDefinition> = index
            .external_symbols
            .iter()
            .map(|sym| convert_symbol_to_definition(sym, true, None, &symbol_map))
            .collect();

        // Normalize project root
        let raw_project_root = index
            .metadata
            .as_ref()
            .map(|m| m.project_root.clone())
            .unwrap_or_default();
        let project_root = raw_project_root
            .strip_prefix("file://")
            .unwrap_or(&raw_project_root)
            .to_string();

        Ok(SemanticData {
            project_root,
            documents,
            external_symbols,
        })
    }
}

fn load_scip_index<P: AsRef<std::path::Path>>(path: P) -> Result<scip::Index> {
    use memmap2::Mmap;
    use prost::Message;
    use std::fs::File;

    let file = File::open(path).context("Failed to open SCIP index file")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap SCIP index file")? };
    let index = scip::Index::decode(&mmap[..]).context("Failed to decode SCIP index")?;
    Ok(index)
}

/// Process a single SCIP document
fn process_document(
    doc: &scip::Document,
    symbol_map: &HashMap<SymbolId, scip::SymbolInformation>,
    _external_symbols: &[scip::SymbolInformation],
) -> Result<(Vec<SymbolDefinition>, Vec<SymbolReference>)> {
    // Collect definition locations for finding enclosing symbols
    let mut def_locations: HashMap<SymbolId, (i32, i32, i32, i32)> = HashMap::new();
    let mut definitions: Vec<SymbolDefinition> = Vec::new();

    // First pass: collect all definitions
    let mut occurrence_defs: Vec<(scip::Occurrence, scip::SymbolInformation)> = Vec::new();

    for occ in &doc.occurrences {
        if (occ.symbol_roles & (scip::SymbolRole::Definition as i32)) == 0 {
            continue;
        }
        if occ.symbol.starts_with("local ") {
            continue;
        }

        let sym_info = symbol_map
            .get(&occ.symbol)
            .cloned()
            .unwrap_or_else(|| create_default_symbol_info(&occ.symbol));

        let (start_line, start_col, end_line, end_col) = if !occ.enclosing_range.is_empty() {
            parse_range(&occ.enclosing_range)
        } else {
            parse_range(&occ.range)
        };

        def_locations.insert(
            occ.symbol.clone(),
            (start_line, start_col, end_line, end_col),
        );

        occurrence_defs.push((occ.clone(), sym_info));
    }

    // Second pass: create SymbolDefinitions
    for (occ, sym_info) in occurrence_defs {
        let (start_line, start_col, end_line, end_col) = if !occ.enclosing_range.is_empty() {
            parse_range(&occ.enclosing_range)
        } else {
            parse_range(&occ.range)
        };

        let enclosing = find_enclosing_symbol(
            start_line,
            start_col,
            end_line,
            end_col,
            &def_locations,
            &occ.symbol,
        );

        let def = convert_occurrence_to_definition(&occ, &sym_info, enclosing, doc, symbol_map);
        definitions.push(def);
    }

    // Third pass: collect references
    let mut references: Vec<SymbolReference> = Vec::new();

    for occ in &doc.occurrences {
        if (occ.symbol_roles & (scip::SymbolRole::Definition as i32)) != 0 {
            continue;
        }
        if occ.symbol.is_empty() || occ.symbol.starts_with("local ") {
            continue;
        }

        let (line, col, _, _) = parse_range(&occ.range);

        // Find enclosing symbol
        let enclosing = find_enclosing_symbol(line, col, line, col, &def_locations, "");

        if enclosing.is_none() {
            continue;
        }

        let role = convert_scip_role(occ.symbol_roles, &occ.symbol);

        references.push(SymbolReference {
            target_symbol: occ.symbol.clone(),
            location: SourceLocation {
                file_path: doc.relative_path.clone(),
                line: line as u32,
                column: col as u32,
            },
            enclosing_symbol: enclosing.unwrap(),
            role,
            context: None,
        });
    }

    Ok((definitions, references))
}

/// Convert SCIP occurrence to SymbolDefinition
fn convert_occurrence_to_definition(
    occ: &scip::Occurrence,
    sym_info: &scip::SymbolInformation,
    enclosing: Option<SymbolId>,
    doc: &scip::Document,
    symbol_map: &HashMap<SymbolId, scip::SymbolInformation>,
) -> SymbolDefinition {
    let (name_start, name_col, _name_end, _name_end_col) = parse_range(&occ.range);
    let (body_start, body_col, body_end, body_end_col) = if !occ.enclosing_range.is_empty() {
        parse_range(&occ.enclosing_range)
    } else {
        parse_range(&occ.range)
    };

    let mut kind = convert_scip_kind(sym_info.kind);
    // If SCIP kind is Unknown, try to infer from symbol string
    if matches!(kind, SymbolKind::Unknown) {
        kind = infer_kind_from_symbol(&occ.symbol);
    }
    let details = extract_symbol_details(sym_info, kind.clone(), symbol_map);

    SymbolDefinition {
        symbol_id: occ.symbol.clone(),
        kind,
        name: sym_info.display_name.clone(),
        display_name: occ.symbol.clone(),
        location: SourceLocation {
            file_path: doc.relative_path.clone(),
            line: name_start as u32,
            column: name_col as u32,
        },
        span: SourceSpan {
            start_line: body_start as u32,
            start_column: body_col as u32,
            end_line: body_end as u32,
            end_column: body_end_col as u32,
        },
        enclosing_symbol: enclosing,
        is_external: false,
        documentation: sym_info.documentation.clone(),
        details,
    }
}

/// Convert SCIP SymbolInformation to SymbolDefinition (for external symbols)
fn convert_symbol_to_definition(
    sym_info: &scip::SymbolInformation,
    is_external: bool,
    enclosing: Option<SymbolId>,
    symbol_map: &HashMap<SymbolId, scip::SymbolInformation>,
) -> SymbolDefinition {
    let mut kind = convert_scip_kind(sym_info.kind);
    // If SCIP kind is Unknown, try to infer from symbol string
    if matches!(kind, SymbolKind::Unknown) {
        kind = infer_kind_from_symbol(&sym_info.symbol);
    }
    let details = extract_symbol_details(sym_info, kind.clone(), symbol_map);

    SymbolDefinition {
        symbol_id: sym_info.symbol.clone(),
        kind,
        name: sym_info.display_name.clone(),
        display_name: sym_info.display_name.clone(),
        location: SourceLocation {
            file_path: String::new(),
            line: 0,
            column: 0,
        },
        span: SourceSpan {
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 0,
        },
        enclosing_symbol: if sym_info.enclosing_symbol.is_empty() {
            enclosing
        } else {
            Some(sym_info.enclosing_symbol.clone())
        },
        is_external,
        documentation: sym_info.documentation.clone(),
        details,
    }
}

/// Extract SymbolDetails from SCIP SymbolInformation
fn extract_symbol_details(
    sym_info: &scip::SymbolInformation,
    kind: SymbolKind,
    symbol_map: &HashMap<SymbolId, scip::SymbolInformation>,
) -> SymbolDetails {
    match kind {
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor => {
            let func_details = extract_function_details(sym_info, symbol_map);
            SymbolDetails::Function(func_details)
        }
        SymbolKind::Variable | SymbolKind::Field | SymbolKind::Constant => {
            let var_details = extract_variable_details(sym_info);
            SymbolDetails::Variable(var_details)
        }
        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Trait
        | SymbolKind::Protocol
        | SymbolKind::TypeAlias => {
            let type_details = extract_type_details(sym_info);
            SymbolDetails::Type(type_details)
        }
        SymbolKind::Parameter => {
            // Parameter details are handled by the parent function
            SymbolDetails::None
        }
        _ => SymbolDetails::None,
    }
}

/// Extract function details from SCIP symbol info
fn extract_function_details(
    sym_info: &scip::SymbolInformation,
    _symbol_map: &HashMap<SymbolId, scip::SymbolInformation>,
) -> FunctionDetails {
    let mut details = FunctionDetails::default();

    // Extract modifiers from signature or kind
    // SCIP kind values: AbstractMethod = 66
    details.modifiers.is_abstract = sym_info.kind == 66;

    // Parse parameters from signature documentation if available
    if let Some(ref sig_doc) = sym_info.signature_documentation {
        let (params, return_type) = parse_signature(&sig_doc.text);
        details.parameters = params;
        if let Some(ret) = return_type {
            details.return_types = vec![ret];
        }
    }

    // Extract return type from relationships
    for rel in &sym_info.relationships {
        if rel.is_type_definition {
            // This might be return type or parameter type
            // For now, use first type definition as return type if not already set
            if details.return_types.is_empty() {
                details.return_types = vec![rel.symbol.clone()];
            }
        }
    }

    // Check if constructor (SCIP kind: Constructor = 9)
    details.modifiers.is_constructor = sym_info.kind == 9;

    // Check visibility
    details.modifiers.visibility = infer_visibility(&sym_info.symbol);

    details
}

/// Extract variable details
fn extract_variable_details(sym_info: &scip::SymbolInformation) -> VariableDetails {
    VariableDetails {
        var_type: None,                  // Will be filled from relationships if available
        mutability: Mutability::Mutable, // Default, may be refined by language
        variable_kind: VariableKind::Global,
        visibility: infer_visibility(&sym_info.symbol),
    }
}

/// Extract type details
fn extract_type_details(sym_info: &scip::SymbolInformation) -> TypeDetails {
    // SCIP kind values: Interface = 21, Enum = 11, Struct = 49, Trait = 53
    let kind = match sym_info.kind {
        21 => TypeKind::Interface,
        11 => TypeKind::Enum,
        49 => TypeKind::Struct,
        53 => TypeKind::Trait,
        _ => TypeKind::Class,
    };

    let is_abstract = kind == TypeKind::Interface || kind == TypeKind::Trait;

    let mut details = TypeDetails {
        kind,
        is_abstract,
        is_final: false,
        visibility: infer_visibility(&sym_info.symbol),
        type_params: Vec::new(),
        fields: Vec::new(),
        implements: Vec::new(),
        inherits: Vec::new(),
    };

    // Extract inheritance/implementations from relationships
    for rel in &sym_info.relationships {
        if rel.is_implementation {
            details.implements.push(rel.symbol.clone());
        }
        if rel.is_reference {
            // Could be inheritance
            details.inherits.push(rel.symbol.clone());
        }
    }

    details
}

/// Parse function signature to extract parameters
fn parse_signature(sig: &str) -> (Vec<ParameterInfo>, Option<SymbolId>) {
    let mut params = Vec::new();
    let mut return_type = None;

    // Simple heuristic parsing - look for patterns like "(a: Type, b: Type) -> ReturnType"
    if let Some(params_start) = sig.find('(')
        && let Some(params_end) = sig.find(')') {
            let params_str = &sig[params_start + 1..params_end];
            for param in params_str.split(',') {
                let param = param.trim();
                if param.is_empty() {
                    continue;
                }

                let (name, param_type) = if let Some(colon_pos) = param.find(':') {
                    let name = param[..colon_pos].trim().to_string();
                    let type_str = param[colon_pos + 1..].trim().to_string();
                    (name, Some(type_str))
                } else {
                    (param.to_string(), None)
                };

                params.push(ParameterInfo {
                    name,
                    param_type,
                    has_default: param.contains('='),
                    is_variadic: param.contains("..."),
                });
            }
        }

    // Look for return type arrow
    if let Some(arrow_pos) = sig.find("->") {
        let ret = sig[arrow_pos + 2..].trim();
        if !ret.is_empty() && ret != ":" {
            return_type = Some(ret.trim_end_matches(':').trim().to_string());
        }
    }

    (params, return_type)
}

/// Convert SCIP SymbolInformation kind to our SymbolKind
/// SCIP kind values from scip.proto:
/// Class = 7, Method = 26, Function = 17, Constructor = 9,
/// Enum = 11, Interface = 21, Struct = 49, Trait = 53,
/// Variable = 61, Field = 15, Constant = 8, Parameter = 37,
/// Namespace = 30, Module = 29, Package = 35, etc.
fn convert_scip_kind(kind: i32) -> SymbolKind {
    match kind {
        17 => SymbolKind::Function,
        26 => SymbolKind::Method,
        9 => SymbolKind::Constructor,
        7 => SymbolKind::Class,
        21 => SymbolKind::Interface,
        11 => SymbolKind::Enum,
        49 => SymbolKind::Struct,
        53 => SymbolKind::Trait,
        61 => SymbolKind::Variable,
        15 => SymbolKind::Field,
        8 => SymbolKind::Constant,
        37 => SymbolKind::Parameter,
        30 => SymbolKind::Namespace,
        29 => SymbolKind::Module,
        35 => SymbolKind::Package,
        42 => SymbolKind::Protocol,
        55 => SymbolKind::TypeAlias,
        _ => SymbolKind::Unknown,
    }
}

/// Infer SymbolKind from SCIP symbol string format
/// Based on SCIP specification (scip.proto):
/// - <namespace>      ::= <name> '/'
/// - <type>           ::= <name> '#'
/// - <term>           ::= <name> '.'
/// - <meta>           ::= <name> ':'
/// - <macro>          ::= <name> '!'
/// - <method>         ::= <name> '(' (<disambiguator>)? ').'
/// - <type-parameter> ::= '[' <name> ']'
/// - <parameter>      ::= '(' <name> ')'
fn infer_kind_from_symbol(symbol: &str) -> SymbolKind {
    // Method: ends with `).` (not just `.`)
    if symbol.ends_with(").") {
        // Check if it's in a class context (has `#` before the method name)
        if symbol.contains('#') {
            return SymbolKind::Method;
        } else {
            return SymbolKind::Function;
        }
    }

    // Parameter: ends with `)` but not `).`
    // Pattern: `function().(param_name)` or just `(param_name)`
    if symbol.ends_with(')') && !symbol.ends_with(").") {
        return SymbolKind::Parameter;
    }

    // Type parameter: ends with `]`
    if symbol.ends_with(']') {
        return SymbolKind::Parameter; // Treat type parameters as parameters
    }

    // Type: ends with `#`
    if symbol.ends_with('#') {
        return SymbolKind::Class;
    }

    // Meta: ends with `:` (e.g., module __init__)
    if symbol.ends_with(':') {
        return SymbolKind::Module;
    }

    // Macro: ends with `!`
    if symbol.ends_with('!') {
        return SymbolKind::Unknown; // No Macro in new SymbolKind yet
    }

    // Namespace: ends with `/`
    if symbol.ends_with('/') {
        return SymbolKind::Namespace;
    }

    // Term: ends with `.` (variable, constant, or field)
    if symbol.ends_with('.') {
        // If there's a `#` before the term, it's likely a field
        if symbol.contains('#') {
            return SymbolKind::Field;
        } else {
            return SymbolKind::Variable;
        }
    }

    SymbolKind::Unknown
}

/// Convert SCIP symbol role to our ReferenceRole
fn convert_scip_role(role: i32, _symbol: &str) -> ReferenceRole {
    use scip::SymbolRole;

    if (role & (SymbolRole::WriteAccess as i32)) != 0 {
        ReferenceRole::Write
    } else if (role & (SymbolRole::ReadAccess as i32)) != 0 {
        ReferenceRole::Read
    } else if (role & (SymbolRole::Import as i32)) != 0 {
        ReferenceRole::Import
    } else {
        // Default to Call for function-like symbols, Read otherwise
        ReferenceRole::Call
    }
}

/// Find enclosing symbol for a range
fn find_enclosing_symbol(
    line: i32,
    col: i32,
    end_line: i32,
    end_col: i32,
    def_locations: &HashMap<SymbolId, (i32, i32, i32, i32)>,
    exclude_symbol: &str,
) -> Option<SymbolId> {
    let mut best_enclosing: Option<(SymbolId, i32)> = None; // (symbol, area)

    for (sym, &(s_line, s_col, e_line, e_col)) in def_locations {
        if sym == exclude_symbol {
            continue;
        }

        // Check if this symbol encloses the range
        let encloses = (s_line < line || (s_line == line && s_col <= col))
            && (e_line > end_line || (e_line == end_line && e_col >= end_col));

        if encloses {
            let area = (e_line - s_line) * 1000 + (e_col - s_col);
            match best_enclosing {
                None => best_enclosing = Some((sym.clone(), area)),
                Some((_, best_area)) if area < best_area => {
                    best_enclosing = Some((sym.clone(), area))
                }
                _ => {}
            }
        }
    }

    best_enclosing.map(|(sym, _)| sym)
}

/// Create default symbol info for unknown symbols
fn create_default_symbol_info(symbol: &str) -> scip::SymbolInformation {
    scip::SymbolInformation {
        symbol: symbol.to_string(),
        display_name: symbol.to_string(),
        kind: 0, // Unknown
        documentation: Vec::new(),
        relationships: Vec::new(),
        signature_documentation: None,
        enclosing_symbol: String::new(),
    }
}

/// Infer visibility from symbol string
fn infer_visibility(symbol: &str) -> Visibility {
    // Simple heuristic: symbols starting with underscore are often private
    if symbol.contains("_") && !symbol.contains("__") {
        // Single underscore - might be private
        // This is language-dependent, so we use a simple heuristic
    }
    Visibility::Unspecified
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_signature_simple() {
        let (params, ret) = parse_signature("(x: int, y: str) -> bool");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "x");
        assert_eq!(params[0].param_type, Some("int".to_string()));
        assert_eq!(ret, Some("bool".to_string()));
    }

    #[test]
    fn test_parse_signature_no_types() {
        let (params, ret) = parse_signature("(x, y)");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "x");
        assert_eq!(params[0].param_type, None);
        assert_eq!(ret, None);
    }

    #[test]
    fn test_convert_scip_kind() {
        // SCIP kind values: Function = 17, Class = 7, Method = 26
        assert_eq!(convert_scip_kind(17), SymbolKind::Function);
        assert_eq!(convert_scip_kind(7), SymbolKind::Class);
        assert_eq!(convert_scip_kind(26), SymbolKind::Method);
    }
}
