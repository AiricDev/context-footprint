//! Test fixture generators for integration tests.
#![allow(dead_code)]

use context_footprint::domain::semantic::{
    Definition, DocumentData, Reference, ReferenceRole, SemanticData, SourceRange, SymbolKind,
    SymbolMetadata,
};

fn default_range() -> SourceRange {
    SourceRange {
        start_line: 0,
        start_column: 0,
        end_line: 1,
        end_column: 10,
    }
}

fn metadata(
    symbol: &str,
    display_name: &str,
    kind: SymbolKind,
    documentation: Vec<String>,
    signature: Option<String>,
    is_external: bool,
) -> SymbolMetadata {
    SymbolMetadata {
        symbol: symbol.to_string(),
        kind,
        display_name: display_name.to_string(),
        documentation,
        signature,
        relationships: Vec::new(),
        enclosing_symbol: None,
        is_external,
    }
}

/// Simplest case: one file, two symbols (func_a, func_b), func_a calls func_b.
pub fn create_semantic_data_simple() -> SemanticData {
    let sym_a = "sym::func_a";
    let sym_b = "sym::func_b";

    SemanticData {
        project_root: "/test".into(),
        documents: vec![DocumentData {
            relative_path: "main.py".into(),
            language: "python".into(),
            definitions: vec![
                Definition {
                    symbol: sym_a.to_string(),
                    range: default_range(),
                    enclosing_range: SourceRange {
                        start_line: 0,
                        start_column: 0,
                        end_line: 5,
                        end_column: 20,
                    },
                    metadata: metadata(
                        sym_a,
                        "func_a",
                        SymbolKind::Function,
                        vec!["Doc for A".into()],
                        Some("(x: int) -> int".into()),
                        false,
                    ),
                },
                Definition {
                    symbol: sym_b.to_string(),
                    range: default_range(),
                    enclosing_range: SourceRange {
                        start_line: 6,
                        start_column: 0,
                        end_line: 10,
                        end_column: 20,
                    },
                    metadata: metadata(sym_b, "func_b", SymbolKind::Function, vec![], None, false),
                },
            ],
            references: vec![Reference {
                symbol: sym_b.to_string(),
                range: default_range(),
                enclosing_symbol: sym_a.to_string(),
                role: ReferenceRole::Call,
            }],
        }],
        external_symbols: vec![],
    }
}

/// Two documents: main.py (func_main) and utils.py (func_util). func_main calls func_util.
pub fn create_semantic_data_two_files() -> SemanticData {
    let sym_main = "sym::main::func_main";
    let sym_util = "sym::utils::func_util";

    SemanticData {
        project_root: "/test".into(),
        documents: vec![
            DocumentData {
                relative_path: "main.py".into(),
                language: "python".into(),
                definitions: vec![Definition {
                    symbol: sym_main.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(
                        sym_main,
                        "func_main",
                        SymbolKind::Function,
                        vec!["Main".into()],
                        Some("() -> ()".into()),
                        false,
                    ),
                }],
                references: vec![Reference {
                    symbol: sym_util.to_string(),
                    range: default_range(),
                    enclosing_symbol: sym_main.to_string(),
                    role: ReferenceRole::Call,
                }],
            },
            DocumentData {
                relative_path: "utils.py".into(),
                language: "python".into(),
                definitions: vec![Definition {
                    symbol: sym_util.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(
                        sym_util,
                        "func_util",
                        SymbolKind::Function,
                        vec![],
                        None,
                        false,
                    ),
                }],
                references: vec![],
            },
        ],
        external_symbols: vec![],
    }
}

/// Cycle: A -> B -> C -> A.
pub fn create_semantic_data_with_cycle() -> SemanticData {
    let sym_a = "sym::a";
    let sym_b = "sym::b";
    let sym_c = "sym::c";

    SemanticData {
        project_root: "/test".into(),
        documents: vec![DocumentData {
            relative_path: "cycle.py".into(),
            language: "python".into(),
            definitions: vec![
                Definition {
                    symbol: sym_a.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(sym_a, "a", SymbolKind::Function, vec![], None, false),
                },
                Definition {
                    symbol: sym_b.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(sym_b, "b", SymbolKind::Function, vec![], None, false),
                },
                Definition {
                    symbol: sym_c.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(sym_c, "c", SymbolKind::Function, vec![], None, false),
                },
            ],
            references: vec![
                Reference {
                    symbol: sym_b.to_string(),
                    range: default_range(),
                    enclosing_symbol: sym_a.to_string(),
                    role: ReferenceRole::Call,
                },
                Reference {
                    symbol: sym_c.to_string(),
                    range: default_range(),
                    enclosing_symbol: sym_b.to_string(),
                    role: ReferenceRole::Call,
                },
                Reference {
                    symbol: sym_a.to_string(),
                    range: default_range(),
                    enclosing_symbol: sym_c.to_string(),
                    role: ReferenceRole::Call,
                },
            ],
        }],
        external_symbols: vec![],
    }
}

/// Shared mutable state: reader R and writers W1, W2. R reads var V; W1 and W2 write V.
/// Builder should add SharedStateWrite edges R->W1, R->W2.
pub fn create_semantic_data_with_shared_state() -> SemanticData {
    let sym_r = "sym::reader";
    let sym_w1 = "sym::writer1";
    let sym_w2 = "sym::writer2";
    let sym_v = "sym::global_var";

    SemanticData {
        project_root: "/test".into(),
        documents: vec![DocumentData {
            relative_path: "state.py".into(),
            language: "python".into(),
            definitions: vec![
                Definition {
                    symbol: sym_r.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(sym_r, "reader", SymbolKind::Function, vec![], None, false),
                },
                Definition {
                    symbol: sym_w1.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(
                        sym_w1,
                        "writer1",
                        SymbolKind::Function,
                        vec![],
                        None,
                        false,
                    ),
                },
                Definition {
                    symbol: sym_w2.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(
                        sym_w2,
                        "writer2",
                        SymbolKind::Function,
                        vec![],
                        None,
                        false,
                    ),
                },
                Definition {
                    symbol: sym_v.to_string(),
                    range: default_range(),
                    enclosing_range: default_range(),
                    metadata: metadata(
                        sym_v,
                        "global_var",
                        SymbolKind::Variable,
                        vec![],
                        None,
                        false,
                    ),
                },
            ],
            references: vec![
                Reference {
                    symbol: sym_v.to_string(),
                    range: default_range(),
                    enclosing_symbol: sym_r.to_string(),
                    role: ReferenceRole::Read,
                },
                Reference {
                    symbol: sym_v.to_string(),
                    range: default_range(),
                    enclosing_symbol: sym_w1.to_string(),
                    role: ReferenceRole::Write,
                },
                Reference {
                    symbol: sym_v.to_string(),
                    range: default_range(),
                    enclosing_symbol: sym_w2.to_string(),
                    role: ReferenceRole::Write,
                },
            ],
        }],
        external_symbols: vec![],
    }
}

/// Helper to build a MockSourceReader that has file contents for all documents in the semantic data.
/// Caller can pass the SemanticData and optionally override content per path.
pub fn source_reader_for_semantic_data(
    data: &SemanticData,
    default_content: &str,
) -> super::mock::MockSourceReader {
    let mut reader = super::mock::MockSourceReader::new();
    for doc in &data.documents {
        let path = std::path::Path::new(&data.project_root).join(&doc.relative_path);
        reader.add_file(path, default_content);
    }
    reader
}
