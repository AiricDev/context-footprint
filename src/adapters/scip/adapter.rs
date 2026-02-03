use crate::adapters::scip::parser::{
    find_enclosing_definition, parameter_name_from_symbol, parent_function_from_parameter_symbol,
    parse_range,
};
use crate::domain::ports::SemanticDataSource;
use crate::domain::semantic::{
    Definition, DocumentData, Parameter, Reference, ReferenceRole, Relationship, RelationshipKind,
    SemanticData, SourceRange, SymbolIndex, SymbolKind, SymbolMetadata,
};
use crate::scip;
use anyhow::{Context, Result};

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

        // Convert external symbols
        let external_symbols = index
            .external_symbols
            .iter()
            .map(|sym| convert_symbol_info(sym, true))
            .collect();

        // Create a global symbol lookup map
        let mut symbol_map = std::collections::HashMap::new();
        for sym in &index.external_symbols {
            symbol_map.insert(sym.symbol.clone(), convert_symbol_info(sym, true));
        }
        for doc in &index.documents {
            for sym in &doc.symbols {
                symbol_map.insert(sym.symbol.clone(), convert_symbol_info(sym, false));
            }
        }

        // Process each document
        let documents: Vec<DocumentData> = index
            .documents
            .iter()
            .map(|doc| {
                let (definitions, references) =
                    partition_occurrences_with_map(doc, &symbol_map);

                // Collect parameters by parent: Parameter definitions are merged into function metadata, not kept as separate definitions
                let parameter_by_parent =
                    build_parameter_by_parent(&definitions);

                // Exclude Parameter from document.definitions; they are attached to function metadata.parameters
                let mut definitions: Vec<Definition> = definitions
                    .into_iter()
                    .filter(|d| d.metadata.kind != SymbolKind::Parameter)
                    .collect();

                // Enrich function-like definitions with parsed parameters and return type (language-specific)
                for def in &mut definitions {
                    enrich_function_signature(&mut def.metadata, &doc.language);
                }

                // Attach parameters from SCIP Parameter definitions to function metadata (overrides signature when present)
                for def in &mut definitions {
                    if let Some(params) = parameter_by_parent.get(&def.symbol) {
                        def.metadata.parameters = params.clone();
                    }
                }

                DocumentData {
                    relative_path: doc.relative_path.clone(),
                    language: doc.language.clone(),
                    definitions,
                    references,
                }
            })
            .collect();

        // `Metadata.project_root` is often a `file://` URI.
        // Downstream code expects a filesystem path, so normalize here.
        let raw_project_root = index
            .metadata
            .as_ref()
            .map(|m| m.project_root.clone())
            .unwrap_or_default();
        let normalized_project_root = raw_project_root
            .strip_prefix("file://")
            .unwrap_or(raw_project_root.as_str())
            .to_string();

        let symbol_index = SymbolIndex::from_definitions(&documents);

        let mut semantic_data = SemanticData {
            project_root: normalized_project_root,
            documents,
            external_symbols,
            symbol_index,
        };

        // Enrich semantic data with language-specific inferences
        enrich_semantic_data(&mut semantic_data)?;

        Ok(semantic_data)
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

/// Build a map from parent function symbol to ordered parameters from Parameter definitions.
/// Used to merge Parameter definitions into function metadata.parameters so they are not
/// exposed as separate entries in document.definitions.
fn build_parameter_by_parent(definitions: &[Definition]) -> std::collections::HashMap<String, Vec<Parameter>> {
    let mut parameter_by_parent: std::collections::HashMap<String, Vec<Parameter>> =
        std::collections::HashMap::new();
    for def in definitions {
        if def.metadata.kind != SymbolKind::Parameter {
            continue;
        }
        let Some(ref parent) = def.metadata.enclosing_symbol else {
            continue;
        };
        let mut param_type = None;
        for rel in &def.metadata.relationships {
            if matches!(rel.kind, RelationshipKind::TypeDefinition) {
                param_type = Some(rel.target_symbol.clone());
                break;
            }
        }
        // Name: SCIP often leaves display_name empty for parameters; parse from symbol (e.g. ...#emit_usage().(event) → "event")
        let name = parameter_name_from_symbol(&def.symbol)
            .unwrap_or_else(|| def.metadata.display_name.clone());
        let param = Parameter {
            name,
            param_type,
        };
        parameter_by_parent
            .entry(parent.clone())
            .or_default()
            .push(param);
    }
    parameter_by_parent
}

fn partition_occurrences_with_map(
    doc: &scip::Document,
    symbol_map: &std::collections::HashMap<String, SymbolMetadata>,
) -> (Vec<Definition>, Vec<Reference>) {
    let mut definitions = Vec::new();
    let mut references = Vec::new();

    // First collect all definitions (for finding enclosing_symbol)
    let defs: Vec<(Vec<i32>, &str)> = doc
        .occurrences
        .iter()
        .filter(|occ| (occ.symbol_roles & (scip::SymbolRole::Definition as i32)) != 0)
        .filter(|occ| !occ.symbol.is_empty())
        .map(|occ| {
            let range = if !occ.enclosing_range.is_empty() {
                &occ.enclosing_range
            } else {
                &occ.range
            };
            (range.clone(), occ.symbol.as_str())
        })
        .collect();

    for occ in &doc.occurrences {
        if (occ.symbol_roles & (scip::SymbolRole::Definition as i32)) != 0 {
            // Skip local symbols: they are document-local and not meaningful for CF
            if occ.symbol.starts_with("local ") {
                continue;
            }
            // Find corresponding SymbolInformation in global map
            let mut metadata = symbol_map
                .get(&occ.symbol)
                .cloned()
                .unwrap_or_else(|| create_default_metadata(&occ.symbol));

            // In SCIP, Function and Parameter are independent symbols; the relationship is encoded
            // in the parameter symbol string (e.g. `pkg . foo().(x)` → parent `pkg . foo().`).
            if metadata.kind == SymbolKind::Parameter {
                if metadata.enclosing_symbol.is_none() {
                    if let Some(parent) = parent_function_from_parameter_symbol(&occ.symbol) {
                        metadata.enclosing_symbol = Some(parent);
                    }
                }
                if metadata.display_name == occ.symbol {
                    if let Some(name) = parameter_name_from_symbol(&occ.symbol) {
                        metadata.display_name = name;
                    }
                }
            }

            let (start_line, start_col, end_line, end_col) = parse_range(&occ.range);
            let (encl_start_line, encl_start_col, encl_end_line, encl_end_col) =
                if !occ.enclosing_range.is_empty() {
                    parse_range(&occ.enclosing_range)
                } else {
                    parse_range(&occ.range)
                };

            definitions.push(Definition {
                symbol: occ.symbol.clone(),
                range: SourceRange {
                    start_line: start_line as u32,
                    start_column: start_col as u32,
                    end_line: end_line as u32,
                    end_column: end_col as u32,
                },
                enclosing_range: SourceRange {
                    start_line: encl_start_line as u32,
                    start_column: encl_start_col as u32,
                    end_line: encl_end_line as u32,
                    end_column: encl_end_col as u32,
                },
                metadata,
            });
        } else if !occ.symbol.is_empty() && !occ.symbol.starts_with("local ") {
            // Find enclosing definition
            let enclosing_symbol = find_enclosing_definition(&occ.range, &defs)
                .unwrap_or("")
                .to_string();

            let (start_line, start_col, end_line, end_col) = parse_range(&occ.range);

            references.push(Reference {
                symbol: occ.symbol.clone(),
                range: SourceRange {
                    start_line: start_line as u32,
                    start_column: start_col as u32,
                    end_line: end_line as u32,
                    end_column: end_col as u32,
                },
                enclosing_symbol,
                role: convert_role(occ.symbol_roles),
            });
        }
    }

    (definitions, references)
}

fn convert_symbol_info(sym: &scip::SymbolInformation, is_external: bool) -> SymbolMetadata {
    let mut kind = convert_symbol_kind(sym.kind() as i32);

    // If kind is Unknown, try to infer from symbol string
    if matches!(kind, SymbolKind::Unknown) {
        kind = infer_kind_from_symbol(&sym.symbol);
    }

    let relationships = sym
        .relationships
        .iter()
        .map(|rel| Relationship {
            target_symbol: rel.symbol.clone(),
            kind: convert_relationship_kind(rel),
        })
        .collect();

    SymbolMetadata {
        symbol: sym.symbol.clone(),
        kind,
        display_name: sym.display_name.clone(),
        documentation: sym.documentation.clone(),
        signature: sym.signature_documentation.as_ref().map(|d| d.text.clone()),
        parameters: Vec::new(), // filled by enrich_function_signature per document
        return_type: None,
        relationships,
        enclosing_symbol: if sym.enclosing_symbol.is_empty() {
            None
        } else {
            Some(sym.enclosing_symbol.clone())
        },
        is_external,
        throws: vec![],
    }
}

fn create_default_metadata(symbol: &str) -> SymbolMetadata {
    // Infer kind from SCIP symbol format
    let kind = infer_kind_from_symbol(symbol);

    SymbolMetadata {
        symbol: symbol.to_string(),
        kind,
        display_name: symbol.to_string(),
        documentation: Vec::new(),
        signature: None,
        parameters: Vec::new(),
        return_type: None,
        relationships: Vec::new(),
        enclosing_symbol: None,
        is_external: false,
        throws: vec![],
    }
}

/// Enrich function-like definitions with parsed parameters and return type from signature (language-specific).
fn enrich_function_signature(metadata: &mut SymbolMetadata, language: &str) {
    use crate::domain::semantic::SymbolKind;

    let function_like = matches!(
        metadata.kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constructor
            | SymbolKind::StaticMethod
            | SymbolKind::AbstractMethod
    );
    if !function_like {
        return;
    }

    let (parameters, return_type) =
        parse_function_signature_for_language(metadata.signature.as_deref(), language);
    metadata.parameters = parameters;
    metadata.return_type = return_type;
}

/// Parse function signature string per language. Returns (parameters, return_type).
/// Other languages can be added here; domain stays language-agnostic.
fn parse_function_signature_for_language(
    signature: Option<&str>,
    language: &str,
) -> (Vec<Parameter>, Option<String>) {
    match language.to_lowercase().as_str() {
        "python" => parse_function_signature_python(signature),
        _ => (Vec::new(), None),
    }
}

/// Python-style signature: "() -> int", "(x: int) -> int", "(x, y)" (no types).
fn parse_function_signature_python(signature: Option<&str>) -> (Vec<Parameter>, Option<String>) {
    let signature = match signature {
        Some(s) if !s.is_empty() => s,
        _ => return (Vec::new(), None),
    };

    let (params_part, return_part) = match signature.split_once("->") {
        Some((params, ret)) => (params.trim(), Some(ret.trim())),
        None => (signature.trim(), None),
    };

    let return_type = return_part.and_then(|ret| {
        let ret = ret.trim().trim_end_matches(':');
        if ret.is_empty() {
            None
        } else {
            Some(ret.to_string())
        }
    });

    let parameters = if params_part.starts_with('(') && params_part.contains(')') {
        let params_content = params_part
            .trim_start_matches('(')
            .split(')')
            .next()
            .unwrap_or("")
            .trim();

        if params_content.is_empty() {
            Vec::new()
        } else {
            params_content
                .split(',')
                .map(|param| {
                    let param = param.trim();
                    let (name, param_type) = match param.split_once(':') {
                        Some((name, type_str)) => {
                            let type_str = type_str.trim();
                            (
                                name.trim().to_string(),
                                if type_str.is_empty() {
                                    None
                                } else {
                                    Some(type_str.to_string())
                                },
                            )
                        }
                        None => (param.to_string(), None),
                    };
                    Parameter { name, param_type }
                })
                .collect()
        }
    } else {
        Vec::new()
    };

    (parameters, return_type)
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
    // Check the last descriptor pattern by examining the end of the symbol

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
        return SymbolKind::Macro;
    }

    // Namespace: ends with `/`
    if symbol.ends_with('/') {
        return SymbolKind::Namespace;
    }

    // Term: ends with `.` (variable, constant, or field)
    // Need to distinguish between field and variable based on context
    if symbol.ends_with('.') {
        // If there's a `#` before the term, it's likely a field
        // Pattern: `Class#field.`
        if symbol.contains('#') {
            return SymbolKind::Field;
        } else {
            // Module-level variable or constant
            return SymbolKind::Variable;
        }
    }

    // Default: Unknown
    SymbolKind::Unknown
}

fn convert_symbol_kind(kind: i32) -> SymbolKind {
    // SCIP Kind is an enum represented as i32
    // Match against the enum values from scip.proto
    match kind {
        17 => SymbolKind::Function,       // Function
        26 => SymbolKind::Method,         // Method
        9 => SymbolKind::Constructor,     // Constructor
        80 => SymbolKind::StaticMethod,   // StaticMethod
        66 => SymbolKind::AbstractMethod, // AbstractMethod
        7 => SymbolKind::Class,           // Class
        21 => SymbolKind::Interface,      // Interface
        49 => SymbolKind::Struct,         // Struct
        11 => SymbolKind::Enum,           // Enum
        55 => SymbolKind::TypeAlias,      // TypeAlias
        53 => SymbolKind::Trait,          // Trait
        42 => SymbolKind::Protocol,       // Protocol
        61 => SymbolKind::Variable,       // Variable
        15 => SymbolKind::Field,          // Field
        8 => SymbolKind::Constant,        // Constant
        37 => SymbolKind::Parameter,      // Parameter
        30 => SymbolKind::Namespace,      // Namespace
        29 => SymbolKind::Module,         // Module
        35 => SymbolKind::Package,        // Package
        25 => SymbolKind::Macro,          // Macro
        _ => SymbolKind::Unknown,
    }
}

fn convert_relationship_kind(rel: &scip::Relationship) -> RelationshipKind {
    if rel.is_implementation {
        RelationshipKind::Implements
    } else if rel.is_type_definition {
        RelationshipKind::TypeDefinition
    } else if rel.is_reference {
        RelationshipKind::References
    } else {
        RelationshipKind::Inherits // Default assumption
    }
}

fn convert_role(symbol_roles: i32) -> ReferenceRole {
    use scip::SymbolRole::*;

    if (symbol_roles & (WriteAccess as i32)) != 0 {
        ReferenceRole::Write
    } else if (symbol_roles & (ReadAccess as i32)) != 0 {
        ReferenceRole::Read
    } else if (symbol_roles & (Import as i32)) != 0 {
        ReferenceRole::Import
    } else {
        // Need to infer from context - default to Call for now
        ReferenceRole::Call
    }
}

/// Enrich semantic data with language-specific inferences
/// This handles cases where the SCIP indexer doesn't provide complete relationship information
fn enrich_semantic_data(data: &mut SemanticData) -> Result<()> {
    for document in &mut data.documents {
        // Determine language by file extension if language field is empty
        let language = if !document.language.is_empty() {
            document.language.as_str()
        } else if document.relative_path.ends_with(".py") {
            "python"
        } else {
            ""
        };

        if language == "python" {
            enrich_python_return_types(document, &data.project_root)?;
        }

        // Fallback: parse return type from documentation when no ref/signature (e.g. "-> None" with no symbol ref)
        for def in &mut document.definitions {
            if !matches!(
                def.metadata.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Constructor
                    | SymbolKind::StaticMethod
                    | SymbolKind::AbstractMethod
            ) {
                continue;
            }
            if def.metadata.return_type.is_none() {
                if let Some(rt) = parse_return_type_from_python_doc(&def.metadata) {
                    def.metadata.return_type = Some(rt);
                }
            }
        }

        // Assign function's TypeDefinition relationships to parameters (indexer often puts param types on the method)
        for def in &mut document.definitions {
            assign_param_types_from_relationships(def);
        }

        // Fallback: parse param types from documentation when relationships didn't provide them
        enrich_param_types_from_documentation(document);
    }
    Ok(())
}

/// Parse return type from Python documentation (e.g. "-> None" or "-> SomeType" in doc/signature).
/// Used when no TypeDefinition ref or signature_documentation is available (e.g. "-> None" has no symbol).
fn parse_return_type_from_python_doc(metadata: &SymbolMetadata) -> Option<String> {
    for doc in &metadata.documentation {
        if let Some(after_arrow) = doc.split("->").nth(1) {
            let ret = after_arrow
                .split(&[':', '\n'][..])
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if let Some(r) = ret {
                return Some(r.to_string());
            }
        }
    }
    None
}

/// Parse parameter types from Python documentation (e.g. "event: UsageEvent") and resolve
/// type names to symbols using document references. Used when the indexer doesn't attach
/// TypeDefinition to the method or to parameter symbols.
fn enrich_param_types_from_documentation(doc: &mut DocumentData) {
    use crate::domain::semantic::SymbolKind;

    let type_symbols: std::collections::HashSet<String> = doc
        .references
        .iter()
        .map(|r| r.symbol.clone())
        .collect();

    for def in &mut doc.definitions {
        let function_like = matches!(
            def.metadata.kind,
            SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Constructor
                | SymbolKind::StaticMethod
                | SymbolKind::AbstractMethod
        );
        if !function_like || def.metadata.parameters.is_empty() {
            continue;
        }
        for param in &mut def.metadata.parameters {
            if param.name == "self" || param.param_type.is_some() {
                continue;
            }
            for doc_str in &def.metadata.documentation {
                let pattern = format!("{}: ", param.name);
                if let Some(after_colon) = doc_str.find(&pattern).and_then(|_| {
                    doc_str.split(&pattern).nth(1).and_then(|s| {
                        s.split(&[' ', '\n', ',', ')', ']', '}']).next().map(str::trim)
                    })
                }) {
                    let type_name = after_colon;
                    if type_name.is_empty() {
                        continue;
                    }
                    // Use the type symbol (e.g. .../UsageEvent#), not a member (e.g. .../UsageEvent#user_id.)
                    let type_suffix = format!("/{}#", type_name);
                    let symbol = type_symbols
                        .iter()
                        .find(|sym| sym.ends_with(&type_suffix))
                        .cloned()
                        .or_else(|| {
                            // Fallback: derive type from a member reference (e.g. .../UsageEvent#user_id. -> .../UsageEvent#)
                            type_symbols
                                .iter()
                                .find(|sym| sym.contains(&type_suffix))
                                .and_then(|sym| {
                                    sym.find(&type_suffix).map(|i| sym[..i + type_suffix.len()].to_string())
                                })
                        });
                    if let Some(s) = symbol {
                        param.param_type = Some(s);
                        break;
                    }
                }
            }
        }
    }
}

/// Assign function's TypeDefinition relationships to parameters that lack param_type.
/// Indexers often attach parameter type (e.g. UsageEvent) to the method symbol; we assign
/// them to params in order (skip "self", skip params that already have type). Exclude
/// the relationship that matches return_type so it isn't assigned to a param.
fn assign_param_types_from_relationships(def: &mut Definition) {
    use crate::domain::semantic::SymbolKind;

    let function_like = matches!(
        def.metadata.kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constructor
            | SymbolKind::StaticMethod
            | SymbolKind::AbstractMethod
    );
    if !function_like || def.metadata.parameters.is_empty() {
        return;
    }

    let return_type = def.metadata.return_type.as_ref();
    let type_defs: Vec<String> = def
        .metadata
        .relationships
        .iter()
        .filter(|r| matches!(r.kind, RelationshipKind::TypeDefinition))
        .map(|r| r.target_symbol.clone())
        .filter(|sym| Some(sym) != return_type)
        .collect();

    let mut type_idx = 0;
    for param in &mut def.metadata.parameters {
        if param.name == "self" || param.param_type.is_some() {
            continue;
        }
        if type_idx < type_defs.len() {
            param.param_type = Some(type_defs[type_idx].clone());
            type_idx += 1;
        }
    }
}

/// Enrich Python function definitions with return type relationships
///
/// SCIP-python doesn't always generate TypeDefinition relationships for return types.
/// This function infers return types by analyzing occurrences and source code patterns.
fn enrich_python_return_types(doc: &mut DocumentData, project_root: &str) -> Result<()> {
    use std::path::Path;

    // Handle file:// URI format in project_root
    let root_path = project_root.strip_prefix("file://").unwrap_or(project_root);

    // Read source file
    let source_path = Path::new(root_path).join(&doc.relative_path);
    let source_code = std::fs::read_to_string(&source_path)
        .context(format!("Failed to read source file: {:?}", source_path))?;
    let lines: Vec<&str> = source_code.lines().collect();

    // Process each function definition
    for definition in &mut doc.definitions {
        // Only process functions
        if !matches!(
            definition.metadata.kind,
            SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Constructor
                | SymbolKind::StaticMethod
                | SymbolKind::AbstractMethod
        ) {
            continue;
        }

        // Find return type candidates from references in this document
        for reference in &doc.references {
            // Must be enclosed by this function
            if reference.enclosing_symbol != definition.symbol {
                continue;
            }

            // Must be a type usage (or Read, which scip-python uses for type annotations)
            if !matches!(
                reference.role,
                ReferenceRole::TypeUsage | ReferenceRole::Call | ReferenceRole::Read
            ) {
                continue;
            }

            // Check if this is a return type annotation using Python syntax patterns
            // This will filter out non-return-type references by checking for -> and : pattern
            if is_python_return_type_annotation(reference, definition, &lines) {
                // Check if we already have this relationship
                let already_exists = definition.metadata.relationships.iter().any(|r| {
                    r.target_symbol == reference.symbol
                        && matches!(r.kind, RelationshipKind::TypeDefinition)
                });

                if !already_exists {
                    definition.metadata.relationships.push(Relationship {
                        target_symbol: reference.symbol.clone(),
                        kind: RelationshipKind::TypeDefinition,
                    });
                }

                // Set return_type in semantic data so debug output and downstream see it
                if definition.metadata.return_type.is_none() {
                    definition.metadata.return_type = Some(reference.symbol.clone());
                }

                // Only one return type per function
                break;
            }
        }
    }

    Ok(())
}

/// Check if a type reference is a Python return type annotation
///
/// Python return type syntax: `def func(...) -> ReturnType:`
/// We only accept a reference that appears *after* "->" on the line, so parameter
/// type annotations (e.g. `event: UsageEvent`) are not mistaken for return type.
fn is_python_return_type_annotation(
    type_ref: &Reference,
    function_def: &Definition,
    lines: &[&str],
) -> bool {
    // Must be in the signature area (within a few lines of function definition)
    let line_num = type_ref.range.start_line as usize;
    let func_line = function_def.range.start_line as usize;

    if line_num < func_line || line_num > func_line + 5 {
        return false;
    }

    // Get the line containing the type reference
    if line_num >= lines.len() {
        return false;
    }

    let line = lines[line_num];

    // Must contain "->" and end with ":" (signature line)
    if !line.contains("->") || !line.trim_end().ends_with(':') {
        return false;
    }

    // Only accept the type that follows "->" (return type), not parameter types before "->"
    let arrow_pos = match line.find("->") {
        Some(p) => p,
        None => return false,
    };
    let after_arrow = line.get(arrow_pos + 2..).unwrap_or("");
    let return_type_start_col = (arrow_pos + 2) as u32
        + (after_arrow.len() - after_arrow.trim_start().len()) as u32;
    if type_ref.range.start_column < return_type_start_col {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use std::io::Write;

    #[test]
    fn test_load_nonexistent_file_returns_error() {
        let adapter = ScipDataSourceAdapter::new("/nonexistent/path/to/index.scip");
        let result = adapter.load();
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("Failed to open") || err_msg.contains("nonexistent"),
            "expected open/decode error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_load_invalid_protobuf_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.scip");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"not valid protobuf content").unwrap();
        drop(f);

        let adapter = ScipDataSourceAdapter::new(&path);
        let result = adapter.load();
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("decode") || err_msg.contains("Failed"),
            "expected decode error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_empty_scip_index_returns_empty_data() {
        let empty_index = scip::Index::default();
        let mut buf = Vec::new();
        empty_index.encode(&mut buf).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.scip");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&buf).unwrap();
        drop(f);

        let adapter = ScipDataSourceAdapter::new(&path);
        let result = adapter.load().unwrap();
        assert!(result.documents.is_empty());
        assert!(result.external_symbols.is_empty());
        assert!(result.project_root.is_empty());
    }

    #[test]
    fn test_infer_kind_from_symbol() {
        assert_eq!(infer_kind_from_symbol("abc/"), SymbolKind::Namespace);
        assert_eq!(infer_kind_from_symbol("abc#"), SymbolKind::Class);
        assert_eq!(infer_kind_from_symbol("abc()."), SymbolKind::Function);
        assert_eq!(
            infer_kind_from_symbol("Class#method()."),
            SymbolKind::Method
        );
        assert_eq!(
            infer_kind_from_symbol("func().(param)"),
            SymbolKind::Parameter
        );
        assert_eq!(infer_kind_from_symbol("func().[T]"), SymbolKind::Parameter);
        assert_eq!(infer_kind_from_symbol("module:"), SymbolKind::Module);
        assert_eq!(infer_kind_from_symbol("macro!"), SymbolKind::Macro);
        assert_eq!(infer_kind_from_symbol("var."), SymbolKind::Variable);
        assert_eq!(infer_kind_from_symbol("Class#field."), SymbolKind::Field);
        assert_eq!(infer_kind_from_symbol("unknown"), SymbolKind::Unknown);
    }

    #[test]
    fn test_convert_symbol_kind() {
        assert_eq!(convert_symbol_kind(17), SymbolKind::Function);
        assert_eq!(convert_symbol_kind(26), SymbolKind::Method);
        assert_eq!(convert_symbol_kind(7), SymbolKind::Class);
        assert_eq!(convert_symbol_kind(61), SymbolKind::Variable);
        assert_eq!(convert_symbol_kind(15), SymbolKind::Field);
        assert_eq!(convert_symbol_kind(37), SymbolKind::Parameter);
        assert_eq!(convert_symbol_kind(30), SymbolKind::Namespace);
        assert_eq!(convert_symbol_kind(29), SymbolKind::Module);
        assert_eq!(convert_symbol_kind(25), SymbolKind::Macro);
        assert_eq!(convert_symbol_kind(999), SymbolKind::Unknown);
    }

    #[test]
    fn test_convert_role() {
        use scip::SymbolRole::*;
        assert_eq!(convert_role(WriteAccess as i32), ReferenceRole::Write);
        assert_eq!(convert_role(ReadAccess as i32), ReferenceRole::Read);
        assert_eq!(convert_role(Import as i32), ReferenceRole::Import);
        assert_eq!(convert_role(0), ReferenceRole::Call);
    }

    #[test]
    fn test_is_python_return_type_annotation() {
        use crate::domain::semantic::{Reference, SourceRange};

        let func_def = Definition {
            symbol: "func".into(),
            range: SourceRange {
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 0,
            },
            enclosing_range: SourceRange {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            metadata: create_default_metadata("func"),
        };

        // "def func() -> MyType:" — MyType starts at column 16 (0-based, after "-> ")
        let type_ref_return = Reference {
            symbol: "MyType".into(),
            range: SourceRange {
                start_line: 0,
                start_column: 16,
                end_line: 0,
                end_column: 22,
            },
            enclosing_symbol: "func".into(),
            role: ReferenceRole::Read,
        };

        let lines = vec!["def func() -> MyType:"];
        assert!(is_python_return_type_annotation(
            &type_ref_return, &func_def, &lines
        ));

        // Parameter type (before "->") must not be treated as return type
        let lines_param_and_return = vec!["def func(event: UsageEvent) -> None:"];
        let type_ref_param = Reference {
            symbol: "UsageEvent".into(),
            range: SourceRange {
                start_line: 0,
                start_column: 15, // "UsageEvent" in "event: UsageEvent"
                end_line: 0,
                end_column: 24,
            },
            enclosing_symbol: "func".into(),
            role: ReferenceRole::Read,
        };
        assert!(!is_python_return_type_annotation(
            &type_ref_param, &func_def, &lines_param_and_return
        ));

        let lines_no_arrow = vec!["def func(x: MyType):"];
        assert!(!is_python_return_type_annotation(
            &type_ref_return,
            &func_def,
            &lines_no_arrow
        ));

        let lines_wrong_line = vec!["def func():", "    return MyType()"];
        let type_ref_wrong = Reference {
            symbol: "MyType".into(),
            range: SourceRange {
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            enclosing_symbol: "func".into(),
            role: ReferenceRole::Read,
        };
        // It's on line 1, which is within func_line + 5, but doesn't have -> and :
        assert!(!is_python_return_type_annotation(
            &type_ref_wrong,
            &func_def,
            &lines_wrong_line
        ));
    }

    /// When a method has TypeDefinition relationships, param_type for non-self params should be assigned.
    #[test]
    fn test_assign_param_types_from_relationships_sets_param_type() {
        use crate::domain::semantic::{Definition, Parameter, Relationship, RelationshipKind, SourceRange};

        let usage_event_symbol = "scip-python python pkg `app.domain.model.usage`/UsageEvent#";
        let method_symbol = "scip-python python pkg `app.adapters.billing`/ArqBillingAdapter#emit_usage().";

        let mut def = Definition {
            symbol: method_symbol.to_string(),
            range: SourceRange {
                start_line: 48,
                start_column: 14,
                end_line: 48,
                end_column: 24,
            },
            enclosing_range: SourceRange {
                start_line: 48,
                start_column: 4,
                end_line: 71,
                end_column: 13,
            },
            metadata: SymbolMetadata {
                symbol: method_symbol.to_string(),
                kind: SymbolKind::Method,
                display_name: String::new(),
                documentation: vec![
                    "```python\nasync def emit_usage(\n  self,\n  event: UsageEvent\n) -> None:\n```".into(),
                ],
                signature: None,
                parameters: vec![
                    Parameter {
                        name: "self".into(),
                        param_type: None,
                    },
                    Parameter {
                        name: "event".into(),
                        param_type: None,
                    },
                ],
                return_type: Some("None".into()),
                relationships: vec![
                    Relationship {
                        target_symbol: "scip-python python pkg `app.domain.ports.billing`/UsageEventPort#emit_usage().".into(),
                        kind: RelationshipKind::Implements,
                    },
                    Relationship {
                        target_symbol: usage_event_symbol.to_string(),
                        kind: RelationshipKind::TypeDefinition,
                    },
                ],
                enclosing_symbol: None,
                is_external: false,
                throws: vec![],
            },
        };

        assign_param_types_from_relationships(&mut def);

        assert_eq!(def.metadata.parameters.len(), 2);
        assert_eq!(def.metadata.parameters[0].name, "self");
        assert!(def.metadata.parameters[0].param_type.is_none(), "self should have no param_type");
        assert_eq!(def.metadata.parameters[1].name, "event");
        assert_eq!(
            def.metadata.parameters[1].param_type.as_deref(),
            Some(usage_event_symbol),
            "event should get param_type from method's TypeDefinition relationship"
        );
    }

    /// When a method has no TypeDefinition on itself but documentation has "event: UsageEvent",
    /// param_type should be filled from document references (type name → symbol).
    #[test]
    fn test_parameters_param_type_from_documentation_when_no_relationship() {
        use crate::domain::semantic::{DocumentData, Definition, Parameter, Reference, ReferenceRole, SourceRange};

        let method_symbol = "pkg/Class#method().";
        let usage_event_symbol = "scip-python python pkg `app.domain.model.usage`/UsageEvent#";

        let mut doc = DocumentData {
            relative_path: "app/adapters/billing.py".into(),
            language: "python".into(),
            definitions: vec![Definition {
                symbol: method_symbol.to_string(),
                range: SourceRange {
                    start_line: 0,
                    start_column: 0,
                    end_line: 0,
                    end_column: 10,
                },
                enclosing_range: SourceRange {
                    start_line: 0,
                    start_column: 0,
                    end_line: 5,
                    end_column: 10,
                },
                metadata: SymbolMetadata {
                    symbol: method_symbol.to_string(),
                    kind: SymbolKind::Method,
                    display_name: String::new(),
                    documentation: vec![
                        "```python\nasync def emit_usage(\n  self,\n  event: UsageEvent\n) -> None:\n```".into(),
                    ],
                    signature: None,
                    parameters: vec![
                        Parameter {
                            name: "self".into(),
                            param_type: None,
                        },
                        Parameter {
                            name: "event".into(),
                            param_type: None,
                        },
                    ],
                    return_type: Some("None".into()),
                    relationships: vec![], // No TypeDefinition on method - indexer didn't attach it
                    enclosing_symbol: None,
                    is_external: false,
                    throws: vec![],
                },
            }],
            references: vec![
                Reference {
                    symbol: usage_event_symbol.to_string(),
                    range: SourceRange {
                        start_line: 2,
                        start_column: 10,
                        end_line: 2,
                        end_column: 20,
                    },
                    enclosing_symbol: method_symbol.to_string(),
                    role: ReferenceRole::TypeUsage,
                },
            ],
        };

        // Enrich param types from documentation using document references (simulate full pipeline)
        enrich_param_types_from_documentation(&mut doc);

        let def = doc.definitions.first().unwrap();
        assert_eq!(def.metadata.parameters.len(), 2);
        assert_eq!(def.metadata.parameters[0].name, "self");
        assert!(def.metadata.parameters[0].param_type.is_none());
        assert_eq!(def.metadata.parameters[1].name, "event");
        assert_eq!(
            def.metadata.parameters[1].param_type.as_deref(),
            Some(usage_event_symbol),
            "event should get param_type from doc (event: UsageEvent) resolved via document reference"
        );
    }

    /// When references include both the type (UsageEvent#) and a member (UsageEvent#user_id.),
    /// param_type must be the type symbol, not the member.
    #[test]
    fn test_param_type_uses_type_symbol_not_member() {
        use crate::domain::semantic::{DocumentData, Definition, Parameter, Reference, ReferenceRole, SourceRange};

        let method_symbol = "pkg/Class#method().";
        let type_symbol = "scip-python python airelay 0.1.0 `app.domain.model.usage`/UsageEvent#";
        let member_symbol = "scip-python python airelay 0.1.0 `app.domain.model.usage`/UsageEvent#user_id.";

        let mut doc = DocumentData {
            relative_path: "app/adapters/billing.py".into(),
            language: "python".into(),
            definitions: vec![Definition {
                symbol: method_symbol.to_string(),
                range: SourceRange {
                    start_line: 0,
                    start_column: 0,
                    end_line: 0,
                    end_column: 10,
                },
                enclosing_range: SourceRange {
                    start_line: 0,
                    start_column: 0,
                    end_line: 10,
                    end_column: 10,
                },
                metadata: SymbolMetadata {
                    symbol: method_symbol.to_string(),
                    kind: SymbolKind::Method,
                    display_name: String::new(),
                    documentation: vec![
                        "```python\nasync def emit_usage(\n  self,\n  event: UsageEvent\n) -> None:\n```".into(),
                    ],
                    signature: None,
                    parameters: vec![
                        Parameter {
                            name: "self".into(),
                            param_type: None,
                        },
                        Parameter {
                            name: "event".into(),
                            param_type: None,
                        },
                    ],
                    return_type: Some("None".into()),
                    relationships: vec![],
                    enclosing_symbol: None,
                    is_external: false,
                    throws: vec![],
                },
            }],
            references: vec![
                Reference {
                    symbol: member_symbol.to_string(),
                    range: SourceRange {
                        start_line: 5,
                        start_column: 0,
                        end_line: 5,
                        end_column: 10,
                    },
                    enclosing_symbol: method_symbol.to_string(),
                    role: ReferenceRole::Read,
                },
            ],
        };

        enrich_param_types_from_documentation(&mut doc);

        let def = doc.definitions.first().unwrap();
        assert_eq!(def.metadata.parameters[1].name, "event");
        assert_eq!(
            def.metadata.parameters[1].param_type.as_deref(),
            Some(type_symbol),
            "param_type must be the type (UsageEvent#), not the member (UsageEvent#user_id.)"
        );
    }
}
