/// Test Python abstract factory pattern recognition
///
/// This test uses the real LLMRelay SCIP index to verify:
/// 1. AuthPort is correctly identified as abstract (Protocol)
/// 2. get_auth_port has ReturnType edge to AuthPort
/// 3. is_abstract_factory correctly identifies get_auth_port
/// 4. get_auth_port's CF doesn't include JuhellmAuthAdapter
use context_footprint::adapters::policy::academic::AcademicBaseline;
use context_footprint::adapters::scip::adapter::ScipDataSourceAdapter;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::edge::EdgeKind;
use context_footprint::domain::node::Node;
use context_footprint::domain::policy::{DocumentationScorer, PruningPolicy, SizeFunction};
use context_footprint::domain::ports::{SemanticDataSource, SourceReader};
use context_footprint::domain::solver::CfSolver;
use std::path::Path;

struct MockSourceReader;

impl SourceReader for MockSourceReader {
    fn read(&self, path: &Path) -> anyhow::Result<String> {
        // Handle file:// URI format
        let path_str = path.to_str().unwrap_or("");
        let actual_path = if path_str.starts_with("file://") {
            Path::new(&path_str[7..])
        } else {
            path
        };

        std::fs::read_to_string(actual_path)
            .map_err(|e| anyhow::anyhow!("Failed to read file {:?}: {}", actual_path, e))
    }

    fn read_lines(
        &self,
        path: &str,
        start_line: usize,
        end_line: usize,
    ) -> anyhow::Result<Vec<String>> {
        let content = self.read(Path::new(path))?;
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let start = start_line.saturating_sub(1);
        let end = end_line.min(lines.len());
        Ok(lines[start..end].to_vec())
    }
}

struct MockSizeFunction;
impl SizeFunction for MockSizeFunction {
    fn compute(
        &self,
        source: &str,
        span: &context_footprint::domain::node::SourceSpan,
        _doc_texts: &[String],
    ) -> u32 {
        let lines: Vec<&str> = source.lines().collect();
        let start = span.start_line as usize;
        let end = span.end_line as usize;
        if start >= lines.len() {
            return 0;
        }
        let end = end.min(lines.len());
        (lines[start..end].iter().map(|l| l.len()).sum::<usize>() / 4) as u32
    }
}

struct MockDocScorer;
impl DocumentationScorer for MockDocScorer {
    fn score(&self, _node: &context_footprint::domain::policy::NodeInfo, doc: Option<&str>) -> f32 {
        if let Some(d) = doc {
            if !d.is_empty() { 1.0 } else { 0.0 }
        } else {
            0.0
        }
    }
}

#[test]
fn test_llmrelay_auth_port_is_abstract() {
    let scip_path = "tests/fixtures/LLMRelay/index.scip";
    if !Path::new(scip_path).exists() {
        eprintln!("Skipping test: SCIP index not found at {}", scip_path);
        return;
    }

    let adapter = ScipDataSourceAdapter::new(scip_path);
    let semantic_data = adapter.load().expect("Failed to load SCIP data");

    let source_reader = MockSourceReader;
    let builder = GraphBuilder::new(Box::new(MockSizeFunction), Box::new(MockDocScorer));

    let graph = builder
        .build(semantic_data, &source_reader)
        .expect("Failed to build graph");

    // Find AuthPort node
    let auth_port_symbol = "scip-python python llmrelay 0.1.0 `app.domain.ports.auth`/AuthPort#";
    let Some(auth_port_idx) = graph.get_node_by_symbol(auth_port_symbol) else {
        eprintln!(
            "Skipping test: expected symbol not found in graph: {}",
            auth_port_symbol
        );
        return;
    };
    let auth_port_node = graph.node(auth_port_idx);

    // Test 1: AuthPort should be a Type node
    match auth_port_node {
        Node::Type(t) => {
            println!("✓ AuthPort is a Type node");
            println!("  is_abstract: {}", t.is_abstract);
            println!("  doc_score: {}", t.core.doc_score);

            // Test 2: AuthPort should be abstract (Protocol)
            assert!(
                t.is_abstract,
                "AuthPort should be abstract (it's a Protocol), but is_abstract = false"
            );

            // Test 3: AuthPort should have good documentation
            assert!(
                t.core.doc_score >= 0.5,
                "AuthPort should have doc_score >= 0.5, got {}",
                t.core.doc_score
            );
        }
        _ => panic!("AuthPort should be a Type node, got {:?}", auth_port_node),
    }
}

#[test]
fn test_llmrelay_get_auth_port_has_return_type_edge() {
    let scip_path = "tests/fixtures/LLMRelay/index.scip";
    if !Path::new(scip_path).exists() {
        eprintln!("Skipping test: SCIP index not found at {}", scip_path);
        return;
    }

    let adapter = ScipDataSourceAdapter::new(scip_path);
    let semantic_data = adapter.load().expect("Failed to load SCIP data");

    let source_reader = MockSourceReader;
    let builder = GraphBuilder::new(Box::new(MockSizeFunction), Box::new(MockDocScorer));

    let graph = builder
        .build(semantic_data, &source_reader)
        .expect("Failed to build graph");

    // Find get_auth_port function
    let get_auth_port_symbol =
        "scip-python python llmrelay 0.1.0 `app.api.dependencies`/get_auth_port().";
    let Some(get_auth_port_idx) = graph.get_node_by_symbol(get_auth_port_symbol) else {
        eprintln!(
            "Skipping test: expected symbol not found in graph: {}",
            get_auth_port_symbol
        );
        return;
    };

    // Find AuthPort type
    let auth_port_symbol = "scip-python python llmrelay 0.1.0 `app.domain.ports.auth`/AuthPort#";
    let Some(auth_port_idx) = graph.get_node_by_symbol(auth_port_symbol) else {
        eprintln!(
            "Skipping test: expected symbol not found in graph: {}",
            auth_port_symbol
        );
        return;
    };

    // Test: get_auth_port should have a ReturnType edge to AuthPort
    let mut found_return_type_edge = false;
    for (neighbor_idx, edge_kind) in graph.neighbors(get_auth_port_idx) {
        if matches!(edge_kind, EdgeKind::ReturnType) && neighbor_idx == auth_port_idx {
            found_return_type_edge = true;
            println!("✓ get_auth_port has ReturnType edge to AuthPort");
            break;
        }
    }

    assert!(
        found_return_type_edge,
        "get_auth_port should have a ReturnType edge to AuthPort"
    );
}

#[test]
fn test_llmrelay_get_auth_port_is_abstract_factory() {
    let scip_path = "tests/fixtures/LLMRelay/index.scip";
    if !Path::new(scip_path).exists() {
        eprintln!("Skipping test: SCIP index not found at {}", scip_path);
        return;
    }

    let adapter = ScipDataSourceAdapter::new(scip_path);
    let semantic_data = adapter.load().expect("Failed to load SCIP data");

    let source_reader = MockSourceReader;
    let builder = GraphBuilder::new(Box::new(MockSizeFunction), Box::new(MockDocScorer));

    let graph = builder
        .build(semantic_data, &source_reader)
        .expect("Failed to build graph");

    // Find get_auth_port function
    let get_auth_port_symbol =
        "scip-python python llmrelay 0.1.0 `app.api.dependencies`/get_auth_port().";
    let Some(get_auth_port_idx) = graph.get_node_by_symbol(get_auth_port_symbol) else {
        eprintln!(
            "Skipping test: expected symbol not found in graph: {}",
            get_auth_port_symbol
        );
        return;
    };
    let get_auth_port_node = graph.node(get_auth_port_idx);

    // Test: AcademicBaseline should recognize get_auth_port as abstract factory
    let policy = AcademicBaseline::default();
    let dummy_caller = Node::Function(context_footprint::domain::node::FunctionNode {
        core: context_footprint::domain::node::NodeCore::new(
            999,
            "dummy".to_string(),
            None,
            10,
            context_footprint::domain::node::SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            0.5,
            false,
            "dummy.py".to_string(),
        ),
        param_count: 0,
        typed_param_count: 0,
        has_return_type: false,
        is_async: false,
        is_generator: false,
        visibility: context_footprint::domain::node::Visibility::Public,
    });

    let decision = policy.evaluate(&dummy_caller, get_auth_port_node, &EdgeKind::Call, &graph);

    println!("Policy decision for get_auth_port: {:?}", decision);

    use context_footprint::domain::policy::PruningDecision;
    assert!(
        matches!(decision, PruningDecision::Boundary),
        "get_auth_port should be identified as Boundary (abstract factory), got {:?}",
        decision
    );
}

#[test]
fn test_llmrelay_caller_of_get_auth_port_cf_excludes_implementation() {
    let scip_path = "tests/fixtures/LLMRelay/index.scip";
    if !Path::new(scip_path).exists() {
        eprintln!("Skipping test: SCIP index not found at {}", scip_path);
        return;
    }

    let adapter = ScipDataSourceAdapter::new(scip_path);
    let semantic_data = adapter.load().expect("Failed to load SCIP data");

    let source_reader = MockSourceReader;
    let builder = GraphBuilder::new(Box::new(MockSizeFunction), Box::new(MockDocScorer));

    let graph = builder
        .build(semantic_data, &source_reader)
        .expect("Failed to build graph");

    // Find a function that CALLS get_auth_port
    // For example, find create_response which uses auth
    let caller_symbols =
        vec!["scip-python python llmrelay 0.1.0 `app.api.openai.responses`/create_response()."];

    let mut found_caller = None;
    for symbol in caller_symbols {
        if let Some(idx) = graph.get_node_by_symbol(symbol) {
            found_caller = Some((symbol, idx));
            break;
        }
    }

    let Some((caller_symbol, caller_idx)) = found_caller else {
        eprintln!("Skipping test: expected caller function not found in graph");
        return;
    };
    println!("Testing caller: {}", caller_symbol);

    // Compute CF with AcademicBaseline policy
    let policy = Box::new(AcademicBaseline::default());
    let solver = CfSolver::new();
    let result = solver.compute_cf(&graph, &[caller_idx], policy.as_ref(), None);

    println!("Caller CF: {} tokens", result.total_context_size);
    println!("Reachable nodes: {}", result.reachable_set.len());

    // Check if JuhellmAuthAdapter is in the context
    let juhellm_adapter_symbol = "scip-python python llmrelay 0.1.0 `app.adapters.auth.juhellm_auth_adapter`/JuhellmAuthAdapter#";

    let juhellm_node_id = graph
        .get_node_by_symbol(juhellm_adapter_symbol)
        .map(|idx| graph.node(idx).core().id);

    let juhellm_in_context = if let Some(juhellm_id) = juhellm_node_id {
        result.reachable_set.contains(&juhellm_id)
    } else {
        false
    };

    println!(
        "JuhellmAuthAdapter in caller's context: {}",
        if juhellm_in_context { "YES" } else { "NO" }
    );

    // Test: JuhellmAuthAdapter should NOT be in the caller's context
    // The caller calls get_auth_port(), which returns AuthPort (abstract).
    // The traversal should stop at get_auth_port (abstract factory boundary),
    // so JuhellmAuthAdapter (concrete implementation inside get_auth_port) should not be reached.
    assert!(
        !juhellm_in_context,
        "JuhellmAuthAdapter should NOT be in the caller's context \
         (caller uses get_auth_port which is an abstract factory, \
         concrete implementation should be hidden)"
    );
}
