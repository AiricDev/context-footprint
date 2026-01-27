use crate::adapters::scip::parser::{find_enclosing_definition, parse_range};
use crate::domain::ports::SemanticDataSource;
use crate::domain::semantic::{
    Definition, DocumentData, Reference, ReferenceRole, Relationship, RelationshipKind,
    SemanticData, SourceRange, SymbolKind, SymbolMetadata,
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
        let documents = index
            .documents
            .iter()
            .map(|doc| {
                let (definitions, references) = partition_occurrences_with_map(doc, &symbol_map);

                DocumentData {
                    relative_path: doc.relative_path.clone(),
                    language: doc.language.clone(),
                    definitions,
                    references,
                }
            })
            .collect();

        let mut semantic_data = SemanticData {
            project_root: index
                .metadata
                .as_ref()
                .map(|m| m.project_root.clone())
                .unwrap_or_default(),
            documents,
            external_symbols,
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
            // Find corresponding SymbolInformation in global map
            let metadata = symbol_map
                .get(&occ.symbol)
                .cloned()
                .unwrap_or_else(|| create_default_metadata(&occ.symbol));

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
        relationships,
        enclosing_symbol: if sym.enclosing_symbol.is_empty() {
            None
        } else {
            Some(sym.enclosing_symbol.clone())
        },
        is_external,
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
        relationships: Vec::new(),
        enclosing_symbol: None,
        is_external: false,
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
        
        match language {
            "python" => enrich_python_return_types(document, &data.project_root)?,
            // Other languages can be added here as needed
            _ => {}
        }
    }
    Ok(())
}

/// Enrich Python function definitions with return type relationships
/// 
/// SCIP-python doesn't always generate TypeDefinition relationships for return types.
/// This function infers return types by analyzing occurrences and source code patterns.
fn enrich_python_return_types(doc: &mut DocumentData, project_root: &str) -> Result<()> {
    use std::path::Path;
    
    // Handle file:// URI format in project_root
    let root_path = if project_root.starts_with("file://") {
        &project_root[7..] // Strip "file://" prefix
    } else {
        project_root
    };
    
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
            if !matches!(reference.role, ReferenceRole::TypeUsage | ReferenceRole::Call | ReferenceRole::Read) {
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
/// We verify this by checking for the presence of `->` and `:` on the same line
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
    
    // Python return type pattern: must contain both "->" and end with ":"
    // Examples:
    //   def func() -> Type:
    //   def func(x: int) -> Type:
    //   ) -> Type:  (multiline signature)
    line.contains("->") && line.trim_end().ends_with(':')
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
}
