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

        // Process each document
        let documents = index
            .documents
            .iter()
            .map(|doc| {
                let (definitions, references) = partition_occurrences(doc);

                DocumentData {
                    relative_path: doc.relative_path.clone(),
                    language: doc.language.clone(),
                    definitions,
                    references,
                }
            })
            .collect();

        Ok(SemanticData {
            project_root: index
                .metadata
                .as_ref()
                .map(|m| m.project_root.clone())
                .unwrap_or_default(),
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

fn partition_occurrences(doc: &scip::Document) -> (Vec<Definition>, Vec<Reference>) {
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
            // Find corresponding SymbolInformation
            let metadata = doc
                .symbols
                .iter()
                .find(|s| s.symbol == occ.symbol)
                .map(|s| convert_symbol_info(s, false))
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
    let kind = convert_symbol_kind(sym.kind() as i32);
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
    SymbolMetadata {
        symbol: symbol.to_string(),
        kind: SymbolKind::Unknown,
        display_name: symbol.to_string(),
        documentation: Vec::new(),
        signature: None,
        relationships: Vec::new(),
        enclosing_symbol: None,
        is_external: false,
    }
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
