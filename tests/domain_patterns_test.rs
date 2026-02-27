/// Test verifying standard Design Patterns against CF Algorithm
///
/// Ensures "Good Abstractions" (Interfaces/Abstract Classes with Docs) act as boundaries.
/// Implements tests for:
/// 1. Strategy Pattern
/// 2. Observer Pattern
/// 3. Facade Pattern
/// 4. Template Method Pattern
/// 5. Adapter Pattern
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::policy::{DocumentationScorer, PruningParams, SizeFunction};
use context_footprint::domain::semantic::{
    DocumentSemantics, Mutability, Parameter, SemanticData, TypeKind,
};
use context_footprint::domain::solver::CfSolver;
use std::sync::Arc;

mod common;
use common::fixtures::{
    call_reference, function_def, read_reference, source_reader_for_semantic_data, type_def,
    variable_def, write_reference,
};

/// Mock services
struct MockSizeFunction;
impl SizeFunction for MockSizeFunction {
    fn compute(
        &self,
        _source: &str,
        _span: &context_footprint::domain::node::SourceSpan,
        _doc_texts: &[String],
    ) -> u32 {
        10 // Uniform size for simplicity
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

/// Helper to build graph and compute CF
fn compute_cf(
    semantic_data: SemanticData,
    start_symbol: &str,
) -> (
    Arc<context_footprint::domain::graph::ContextGraph>,
    context_footprint::domain::solver::CfResult,
) {
    let source_reader = source_reader_for_semantic_data(&semantic_data, "content");
    let builder = GraphBuilder::new(Box::new(MockSizeFunction), Box::new(MockDocScorer));
    let graph = builder
        .build(semantic_data, &source_reader)
        .expect("Failed to build graph");

    let graph_arc = Arc::new(graph);
    let start_idx = graph_arc
        .get_node_by_symbol(start_symbol)
        .expect("Start symbol not found");

    let solver = CfSolver::new(Arc::clone(&graph_arc), PruningParams::academic(0.5));
    let result = solver.compute_cf(&[start_idx], None);

    (graph_arc, result)
}

fn assert_reachable(
    graph: &context_footprint::domain::graph::ContextGraph,
    result: &context_footprint::domain::solver::CfResult,
    symbol: &str,
) {
    let idx = graph
        .get_node_by_symbol(symbol)
        .unwrap_or_else(|| panic!("Symbol {} not in graph", symbol));
    let id = graph.node(idx).core().id;
    assert!(
        result.reachable_set.contains(&id),
        "Symbol {} should be reachable",
        symbol
    );
}

fn assert_not_reachable(
    graph: &context_footprint::domain::graph::ContextGraph,
    result: &context_footprint::domain::solver::CfResult,
    symbol: &str,
) {
    let idx = graph
        .get_node_by_symbol(symbol)
        .unwrap_or_else(|| panic!("Symbol {} not in graph", symbol));
    let id = graph.node(idx).core().id;
    assert!(
        !result.reachable_set.contains(&id),
        "Symbol {} should NOT be reachable",
        symbol
    );
}

/// Strategy Pattern:
/// Client -> Context -> IStrategy (Interface)
/// Client should NOT see ConcreteStrategyA or ConcreteStrategyB
#[test]
fn test_strategy_pattern_boundary() {
    let sym_client = "client_func";
    let sym_context = "Context";
    let sym_strategy_interface = "IStrategy";
    let sym_concrete_a = "ConcreteStrategyA";

    let documents = vec![DocumentSemantics {
        relative_path: "strategy.py".into(),
        language: "python".into(),
        definitions: vec![
            // Client function
            function_def(sym_client, "client", vec![], vec![], None),
            // Context class (represented as func/module for simplicity or just a node)
            // In our graph, classes are types unless they have methods.
            // Let's model Context as a function that takes a strategy.
            function_def(
                sym_context,
                "execute_strategy",
                vec!["Uses strategy".into()], // Documented
                vec![Parameter {
                    name: "strategy".into(),
                    param_type: Some(sym_strategy_interface.into()),
                    ..Default::default()
                }],
                None,
            ),
            // Interface (Type Registry)
            type_def(
                sym_strategy_interface,
                "IStrategy",
                vec!["Strategy Interface".into()],
                TypeKind::Interface,
                true,
            ),
            // Concrete Strategy (Function implementing interface - implicitly via call graph if we wire it)
            // In real code, Context is injected with Concrete.
            // If Context depends ONLY on IStrategy, it shouldn't link to Concrete in the static graph
            // unless we have an instantiation somewhere.
            // Let's assume the Client instantiates Concrete and passes to Context.
            function_def(sym_concrete_a, "strategy_a_impl", vec![], vec![], None),
        ],
        references: vec![
            // Client calls Context
            call_reference(sym_context, sym_client),
            // Client instantiates Concrete (to pass it) - ReferenceRole::Call (constructor)
            call_reference(sym_concrete_a, sym_client),
        ],
    }];

    let _data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    // Note on Strategy Test:
    // If Client instantiates Concrete, Client depends on Concrete.
    // The "Boundary" value of Strategy is that Context doesn't depend on Concrete.
    // So if we start at Context, we shouldn't see Concrete.

    // Test 1: Start from Context (checking isolation)
    // Context depends on IStrategy (Type). It does NOT depend on Concrete.
    // In our graph builder, we don't have "Implementations" edges unless explicitly referenced.
    // So this is naturally true in static analysis unless we have dependency injection wiring.
    // BUT, let's look at the function signature: Context(s: IStrategy).
    // Is IStrategy a boundary?
    // IStrategy is a TYPE. Nodes are FUNCTIONS/VARS.
    // If Context uses methods of IStrategy, those are dynamic dispatches.

    // Let's refine: The CF metric measures "how much code you need to read".
    // If Context calls `s.execute()`, and `s` is IStrategy.
    // Does traversal follow to ConcreteStrategy.execute()?
    // Static graph: NO (unless we have call-graph resolution).
    // This confirms that "Coding to Interface" naturally limits CF in static analysis.

    // What if we test the "Factory" aspect?
    // Factory -> Returns IStrategy.
    // Client -> Calls Factory.
    // Client -> Uses IStrategy.
    // Client should NOT reach ConcreteStrategy.

    let sym_factory = "StrategyFactory";

    let docs_factory = vec![DocumentSemantics {
        relative_path: "factory_pattern.py".into(),
        language: "python".into(),
        definitions: vec![
            // Client
            function_def(sym_client, "client", vec![], vec![], None),
            // Factory Function - Returns IStrategy
            function_def(
                sym_factory,
                "create_strategy",
                vec!["Creates strategy".into()],
                vec![],
                Some(sym_strategy_interface.into()),
            ),
            // Interface
            type_def(
                sym_strategy_interface,
                "IStrategy",
                vec!["Doc".into()],
                TypeKind::Interface,
                true,
            ),
            // Concrete Implementation (inside factory body)
            function_def(sym_concrete_a, "ConcreteImpl", vec![], vec![], None),
        ],
        references: vec![
            // Client calls Factory
            call_reference(sym_factory, sym_client),
            // Factory instantiates Concrete
            call_reference(sym_concrete_a, sym_factory),
        ],
    }];

    let data_factory = SemanticData {
        project_root: "/test".into(),
        documents: docs_factory,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data_factory, sym_client);

    // Factory is called by Client.
    // Factory returns IStrategy (Abstract + Documented).
    // Factory should be a Boundary (Abstract Factory Pattern detection).
    // Therefore, traversal should STOP at Factory.
    // ConcreteImpl (called by Factory) should NOT be reached.

    assert_reachable(&graph, &result, sym_client);
    assert_reachable(&graph, &result, sym_factory);

    // CRITICAL: Concrete Implementation should be hidden
    assert_not_reachable(&graph, &result, sym_concrete_a);
}

/// Observer Pattern:
/// Subject -> notify() -> Observer.update()
/// Observer is Interface.
/// Subject should not reach ConcreteObserver.
#[test]
fn test_observer_pattern_boundary() {
    let sym_subject = "Subject::notify";
    let _sym_observer_interface = "IObserver";
    let sym_concrete_observer = "ConcreteObserver::update";

    // In a static graph, Subject has a list of IObserver.
    // Subject.notify() calls IObserver.update().
    // If IObserver is an Interface, there is no "code" for update().
    // So there is no edge to ConcreteObserver unless we resolve implementations.
    // Since CF is static analysis, this isolation is automatic IF we rely on types.
    //
    // However, if we modeled a "leaky" observer where Subject calls Concrete directly,
    // it would traverse.
    //
    // Let's verify that even if there WAS a link (e.g. some manual wiring),
    // a well-typed boundary stops it.
    // Actually, the graph builder only creates Call edges to resolved symbols.
    // If IObserver.update is abstract, it has no body.
    //
    // Let's treat this as a "Leaky" vs "Strict" comparison.
    // Leaky: Subject calls GenericObserver (Function). GenericObserver calls SpecificObserver.
    // If GenericObserver is well-documented and typed, it blocks SpecificObserver.

    let sym_generic_update = "GenericObserver::update";

    let documents = vec![DocumentSemantics {
        relative_path: "observer.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_subject, "notify", vec![], vec![], None),
            // Intermediate "Interface-like" function (e.g. base class method)
            function_def(
                sym_generic_update,
                "update",
                vec!["Abstract update".into()], // Documented
                vec![Parameter {
                    name: "event".into(),
                    param_type: Some("Event".into()),
                    ..Default::default()
                }],
                Some("void".into()),
            ),
            function_def(sym_concrete_observer, "update_impl", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_generic_update, sym_subject),
            // Simulate a "dynamic dispatch" or "super call" link that might exist in a call graph
            // or just a direct dependency if the code is coupled.
            call_reference(sym_concrete_observer, sym_generic_update),
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_subject);

    // GenericUpdate is Typed + Documented => Boundary.
    // ConcreteObserver (called by GenericUpdate) should be pruned.
    assert_reachable(&graph, &result, sym_subject);
    assert_reachable(&graph, &result, sym_generic_update);
    assert_not_reachable(&graph, &result, sym_concrete_observer);
}

/// Facade Pattern:
/// Client -> Facade -> SubsystemA, SubsystemB
/// Facade is well-documented. Subsystems are details.
/// Traversal should stop at Facade.
#[test]
fn test_facade_pattern_boundary() {
    let sym_client = "Client";
    let sym_facade = "SystemFacade";
    let sym_sub_a = "SubsystemA";
    let sym_sub_b = "SubsystemB";

    let documents = vec![DocumentSemantics {
        relative_path: "facade.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client, "main", vec![], vec![], None),
            // Facade: Public API, Documented, Typed
            function_def(
                sym_facade,
                "simple_interface",
                vec!["Unified interface".into()],
                vec![Parameter {
                    name: "req".into(),
                    param_type: Some("Request".into()),
                    ..Default::default()
                }],
                Some("Response".into()),
            ),
            // Subsystems: Implementation details
            function_def(sym_sub_a, "complex_op_a", vec![], vec![], None),
            function_def(sym_sub_b, "complex_op_b", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_facade, sym_client),
            call_reference(sym_sub_a, sym_facade),
            call_reference(sym_sub_b, sym_facade),
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_client);

    assert_reachable(&graph, &result, sym_client);
    assert_reachable(&graph, &result, sym_facade);

    // Facade hides subsystems
    assert_not_reachable(&graph, &result, sym_sub_a);
    assert_not_reachable(&graph, &result, sym_sub_b);
}

/// Template Method Pattern:
/// Client -> AbstractClass.template_method() -> ConcreteClass.primitive_op()
/// If AbstractClass is well documented, it should be the boundary.
#[test]
fn test_template_method_boundary() {
    let sym_client = "Client";
    let sym_template = "AbstractClass::template_method";
    let sym_primitive = "ConcreteClass::primitive_op";

    let documents = vec![DocumentSemantics {
        relative_path: "template.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client, "run", vec![], vec![], None),
            function_def(
                sym_template,
                "process",
                vec!["Defines algorithm skeleton".into()],
                vec![Parameter {
                    name: "data".into(),
                    param_type: Some("Data".into()),
                    ..Default::default()
                }],
                Some("Result".into()),
            ),
            function_def(sym_primitive, "do_step", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_template, sym_client),
            call_reference(sym_primitive, sym_template), // Template calls primitive
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_client);

    assert_reachable(&graph, &result, sym_client);
    assert_reachable(&graph, &result, sym_template);
    assert_not_reachable(&graph, &result, sym_primitive);
}

/// Adapter Pattern:
/// Client -> Target (Interface)
/// Adapter implements Target, calls Adaptee.
/// This is similar to Strategy/Factory.
/// If we have an "Adapter Function" that converts calls, it acts as a boundary.
#[test]
fn test_adapter_pattern_boundary() {
    let sym_client = "Client";
    let sym_adapter = "Adapter::request"; // The "Wrapper"
    let sym_adaptee = "Adaptee::specific_request"; // The "Wrapped"

    let documents = vec![DocumentSemantics {
        relative_path: "adapter.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client, "main", vec![], vec![], None),
            // Adapter: Well documented wrapper
            function_def(
                sym_adapter,
                "request",
                vec!["Adapts interface".into()],
                vec![],
                Some("void".into()),
            ),
            // Adaptee: The legacy/external code being wrapped
            function_def(sym_adaptee, "specific_req", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_adapter, sym_client),
            call_reference(sym_adaptee, sym_adapter),
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_client);

    assert_reachable(&graph, &result, sym_client);
    assert_reachable(&graph, &result, sym_adapter);
    assert_not_reachable(&graph, &result, sym_adaptee);
}

/// Negative Test: Leaky Abstraction
/// A "Facade" that is NOT documented or NOT typed should be transparent.
#[test]
fn test_leaky_facade_traversed() {
    let sym_client = "Client";
    let sym_bad_facade = "BadFacade";
    let sym_subsystem = "Subsystem";

    let documents = vec![DocumentSemantics {
        relative_path: "leaky.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client, "main", vec![], vec![], None),
            // Bad Facade: No Docs, Missing Types
            function_def(
                sym_bad_facade,
                "do_stuff",
                vec![], // No docs
                vec![Parameter {
                    name: "x".into(),
                    param_type: None,
                    ..Default::default()
                }], // Untyped
                None,   // No return type
            ),
            function_def(sym_subsystem, "worker", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_bad_facade, sym_client),
            call_reference(sym_subsystem, sym_bad_facade),
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_client);

    assert_reachable(&graph, &result, sym_bad_facade);

    // Leaky abstraction -> Subsystem IS reachable
    assert_reachable(&graph, &result, sym_subsystem);
}

/// Singleton Pattern (Global Mutable State):
/// Client -> Singleton.getInstance() -> Global State
/// All writers to the Global State should be included in the context (reverse exploration via incoming Write edges).
/// This "penalizes" the use of global state by exploding the context size.
#[test]
fn test_singleton_global_state_penalty() {
    let sym_client = "Client";
    let sym_singleton_get = "Singleton::getInstance";
    let sym_global_instance = "Singleton::_instance";
    let sym_writer_1 = "SomeWriter1";
    let sym_writer_2 = "SomeWriter2";

    // Scenario:
    // Client reads Singleton (indirectly via getInstance).
    // Writers modify Singleton instance (e.g. reset, reconfigure).
    // Context Footprint should include Writers because Client depends on mutable global state.

    let documents = vec![DocumentSemantics {
        relative_path: "singleton.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client, "main", vec![], vec![], None),
            // Singleton Accessor (reads the global var)
            function_def(
                sym_singleton_get,
                "get_instance",
                vec![],
                vec![],
                Some("Singleton".into()),
            ),
            // Global Variable (Mutable)
            variable_def(
                sym_global_instance,
                "_instance",
                vec![],
                Some("Singleton".into()),
                Mutability::Mutable,
            ),
            // Writers that mutate the global state
            function_def(sym_writer_1, "reset_singleton", vec![], vec![], None),
            function_def(sym_writer_2, "init_singleton", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_singleton_get, sym_client),
            // getInstance READS _instance
            read_reference(sym_global_instance, sym_singleton_get),
            // Writers WRITE _instance
            write_reference(sym_global_instance, sym_writer_1),
            write_reference(sym_global_instance, sym_writer_2),
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_client);

    assert_reachable(&graph, &result, sym_client);
    assert_reachable(&graph, &result, sym_singleton_get);

    // Writers should be reached via reverse exploration (Reader -> Var via Read; Var has incoming Write from W1, W2)
    // sym_singleton_get (Reader) -> sym_writer_1 (Writer)
    assert_reachable(&graph, &result, sym_writer_1);
    assert_reachable(&graph, &result, sym_writer_2);
}

/// Interface Segregation Principle (ISP):
/// ClientA uses FatInterface (calls method A).
/// FatInterface has method A and method B.
/// Implementation implements A and B.
/// If ClientA uses FatInterface, does it "depend" on method B?
/// In static graph, ClientA -> FatInterface.
/// If FatInterface is a boundary, we stop there.
///
/// BUT, let's verify that separating interfaces yields a cleaner graph compared to a "Mega Module".
///
/// Case 1: Fat Interface (One big file/module)
/// Client -> MegaModule (Boundary) -> [ImplA, ImplB, ImplC...]
/// If MegaModule is a boundary, we stop.
///
/// Case 2: Segregated Interfaces
/// Client -> InterfaceA (Boundary).
///
/// The key verification here is:
/// If we have a "Transparent" Fat Interface (e.g. a module that just exports everything without docs/types),
/// Client -> FatModule -> [All Dependencies of FatModule].
///
/// If we have Segregated Modules:
/// Client -> SmallModule -> [Only Dependencies of SmallModule].
///
/// Let's test "Transparent Node" behavior as a proxy for ISP violation.
/// A "Utils" module (Transparent) that imports everything causes high coupling.
#[test]
fn test_transparent_module_coupling_isp_violation() {
    let sym_client = "Client";
    let sym_fat_utils = "god_utils"; // Transparent (no docs, many deps)
    let sym_dep_a = "HeavyDepA";
    let sym_dep_b = "HeavyDepB";

    let documents = vec![DocumentSemantics {
        relative_path: "isp_violation.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client, "main", vec![], vec![], None),
            // Fat Utils function/module acts as a passthrough or aggregator
            function_def(sym_fat_utils, "do_all", vec![], vec![], None), // No docs -> Transparent
            function_def(sym_dep_a, "heavy_a", vec![], vec![], None),
            function_def(sym_dep_b, "heavy_b", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_fat_utils, sym_client),
            call_reference(sym_dep_a, sym_fat_utils), // Fat utils calls A
            call_reference(sym_dep_b, sym_fat_utils), // Fat utils calls B
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_client);

    // Because fat_utils is Transparent, Client pulls in EVERYTHING.
    assert_reachable(&graph, &result, sym_dep_a);
    assert_reachable(&graph, &result, sym_dep_b);
}

#[test]
fn test_segregated_interface_isp_compliance() {
    let sym_client = "Client";
    let sym_small_utils = "lean_utils"; // Transparent but focused
    let sym_dep_a = "HeavyDepA";
    let sym_dep_b = "HeavyDepB"; // Not used by small utils

    let documents = vec![DocumentSemantics {
        relative_path: "isp_compliance.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client, "main", vec![], vec![], None),
            // Lean Utils - only calls A
            function_def(sym_small_utils, "do_a", vec![], vec![], None),
            function_def(sym_dep_a, "heavy_a", vec![], vec![], None),
            function_def(sym_dep_b, "heavy_b", vec![], vec![], None),
        ],
        references: vec![
            call_reference(sym_small_utils, sym_client),
            call_reference(sym_dep_a, sym_small_utils),
            // Dep B is NOT called by lean utils
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    let (graph, result) = compute_cf(data, sym_client);

    assert_reachable(&graph, &result, sym_dep_a);
    // Dep B is not reached -> smaller footprint
    assert_not_reachable(&graph, &result, sym_dep_b);
}

/// Law of Demeter (LoD) / Principle of Least Knowledge:
/// Case A (Train Wreck): Client calls a.get_b().get_c().do_action()
/// Client explicitly references A, B, and C.
/// Context Footprint should be high (A + B + C).
///
/// Case B (Encapsulated): Client calls a.do_action_wrapper()
/// Client only references A. A handles B and C internally.
/// If A is a boundary, Context Footprint should be low (A only).
#[test]
fn test_law_of_demeter_violation_penalty() {
    let sym_client_bad = "ClientBad";
    let sym_client_good = "ClientGood";

    let sym_a = "ClassA";
    let sym_b = "ClassB";
    let sym_c = "ClassC";

    let sym_get_b = "ClassA::get_b";
    let sym_get_c = "ClassB::get_c";
    let sym_do_action = "ClassC::do_action";

    let sym_wrapper = "ClassA::do_action_wrapper";

    let documents = vec![DocumentSemantics {
        relative_path: "lod.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_client_bad, "train_wreck", vec![], vec![], None),
            function_def(sym_client_good, "encapsulated", vec![], vec![], None),
            // Class A
            function_def(sym_a, "ClassA", vec![], vec![], None),
            function_def(sym_get_b, "get_b", vec![], vec![], Some(sym_b.into())), // Returns B
            // Wrapper in A (Encapsulated) - Documented/Typed to act as Boundary
            function_def(
                sym_wrapper,
                "do_action_wrapper",
                vec!["Delegates to C".into()],
                vec![],
                Some("void".into()),
            ),
            // Class B
            function_def(sym_b, "ClassB", vec![], vec![], None),
            function_def(sym_get_c, "get_c", vec![], vec![], Some(sym_c.into())), // Returns C
            // Class C
            function_def(sym_c, "ClassC", vec![], vec![], None),
            function_def(sym_do_action, "do_action", vec![], vec![], None),
        ],
        references: vec![
            // Bad Client: a.get_b().get_c().do_action()
            // It calls get_b (on A), get_c (on B), do_action (on C)
            call_reference(sym_get_b, sym_client_bad),
            call_reference(sym_get_c, sym_client_bad),
            call_reference(sym_do_action, sym_client_bad),
            // Good Client: a.do_action_wrapper()
            call_reference(sym_wrapper, sym_client_good),
            // Wiring for internal calls (just to make graph complete, though not strictly needed for Client reachability if A is boundary)
            call_reference(sym_get_c, sym_wrapper), // Wrapper calls B.get_c
            call_reference(sym_do_action, sym_wrapper), // Wrapper calls C.do_action
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    // Test Bad Client (Train Wreck)
    let (graph, result_bad) = compute_cf(data.clone(), sym_client_bad);

    // Client sees A's method
    assert_reachable(&graph, &result_bad, sym_get_b);
    // Client sees B's method (Violation of LoD causes reachability of neighbor's neighbor)
    assert_reachable(&graph, &result_bad, sym_get_c);
    // Client sees C's method
    assert_reachable(&graph, &result_bad, sym_do_action);

    println!("LoD Violation Size: {}", result_bad.total_context_size);

    // Test Good Client (Encapsulated)
    let (graph_good, result_good) = compute_cf(data, sym_client_good);

    // Client sees A's wrapper
    assert_reachable(&graph_good, &result_good, sym_wrapper);

    // Client should NOT see internal details if Wrapper is a boundary
    // Wrapper is Documented + Typed ("Delegates to C", "void") -> Boundary
    assert_not_reachable(&graph_good, &result_good, sym_get_c);
    assert_not_reachable(&graph_good, &result_good, sym_do_action);

    println!("LoD Compliance Size: {}", result_good.total_context_size);

    assert!(
        result_good.total_context_size < result_bad.total_context_size,
        "Encapsulated code should have smaller footprint than train wreck"
    );
}

/// Mediator Pattern (Star vs Mesh Topology):
/// Case A (Spaghetti): ColleagueA directly calls B, C, D.
/// Context(A) = {A, B, C, D}.
///
/// Case B (Mediator): ColleagueA calls Mediator. Mediator calls B, C, D.
/// Context(A) = {A, Mediator}. (Assuming Mediator is a boundary).
///
/// This verifies that centralizing control effectively decouples components
/// from each other's details, reducing the "local reasoning" load for any single component.
#[test]
fn test_mediator_topology_decoupling() {
    let sym_colleague_a_mesh = "ColleagueA_Mesh";
    let sym_colleague_a_star = "ColleagueA_Star";

    let sym_colleague_b = "ColleagueB";
    let sym_colleague_c = "ColleagueC";
    let sym_colleague_d = "ColleagueD";

    let sym_mediator = "Mediator::broadcast";

    let documents = vec![DocumentSemantics {
        relative_path: "mediator.py".into(),
        language: "python".into(),
        definitions: vec![
            function_def(sym_colleague_a_mesh, "send_mesh", vec![], vec![], None),
            function_def(sym_colleague_a_star, "send_star", vec![], vec![], None),
            function_def(sym_colleague_b, "receive_b", vec![], vec![], None),
            function_def(sym_colleague_c, "receive_c", vec![], vec![], None),
            function_def(sym_colleague_d, "receive_d", vec![], vec![], None),
            // Mediator: Well documented boundary
            function_def(
                sym_mediator,
                "broadcast",
                vec!["Coordinates B,C,D".into()],
                vec![Parameter {
                    name: "msg".into(),
                    param_type: Some("str".into()),
                    ..Default::default()
                }],
                Some("void".into()),
            ),
        ],
        references: vec![
            // Mesh: A calls B, C, D directly
            call_reference(sym_colleague_b, sym_colleague_a_mesh),
            call_reference(sym_colleague_c, sym_colleague_a_mesh),
            call_reference(sym_colleague_d, sym_colleague_a_mesh),
            // Star: A calls Mediator
            call_reference(sym_mediator, sym_colleague_a_star),
            // Mediator calls B, C, D (internal complexity hidden from A)
            call_reference(sym_colleague_b, sym_mediator),
            call_reference(sym_colleague_c, sym_mediator),
            call_reference(sym_colleague_d, sym_mediator),
        ],
    }];

    let data = SemanticData {
        project_root: "/test".into(),
        documents,
        external_symbols: vec![],
    };

    // Test Mesh (Spaghetti)
    let (graph, result_mesh) = compute_cf(data.clone(), sym_colleague_a_mesh);
    assert_reachable(&graph, &result_mesh, sym_colleague_b);
    assert_reachable(&graph, &result_mesh, sym_colleague_c);
    assert_reachable(&graph, &result_mesh, sym_colleague_d);

    let mesh_size = result_mesh.total_context_size;
    println!("Mesh Topology Size: {}", mesh_size);

    // Test Star (Mediator)
    let (graph_star, result_star) = compute_cf(data, sym_colleague_a_star);
    assert_reachable(&graph_star, &result_star, sym_mediator);

    // A should NOT see B, C, D because Mediator acts as a boundary
    assert_not_reachable(&graph_star, &result_star, sym_colleague_b);
    assert_not_reachable(&graph_star, &result_star, sym_colleague_c);
    assert_not_reachable(&graph_star, &result_star, sym_colleague_d);

    let star_size = result_star.total_context_size;
    println!("Star Topology Size: {}", star_size);

    assert!(
        star_size < mesh_size,
        "Mediator topology should reduce local footprint compared to mesh/spaghetti topology"
    );
}
