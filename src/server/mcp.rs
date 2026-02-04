use crate::app::dto::*;
use crate::app::engine::ContextEngine;
use rmcp::{
    Json, ServerHandler, ServiceExt, handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters, model::*, tool, tool_handler, tool_router,
    transport::stdio,
};
use tokio::task::spawn_blocking;

#[derive(Clone)]
pub struct CfMcpServer {
    engine: ContextEngine,
    tool_router: ToolRouter<Self>,
}

impl CfMcpServer {
    pub fn new(engine: ContextEngine) -> Self {
        Self {
            engine,
            tool_router: Self::tool_router(),
        }
    }

    pub async fn serve_stdio(self) -> anyhow::Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

#[tool_router]
impl CfMcpServer {
    #[tool(description = "Compute Context Footprint (CF) for one or more symbols (union).")]
    async fn compute_cf(
        &self,
        params: Parameters<ComputeRequest>,
    ) -> Result<Json<ComputeResponse>, String> {
        let engine = self.engine.clone();
        let req = params.0;
        spawn_blocking(move || engine.compute(req))
            .await
            .map_err(|e| format!("task join error: {e}"))?
            .map(Json)
            .map_err(|e| e.to_string())
    }

    #[tool(description = "Compute CF distribution stats across all nodes.")]
    async fn cf_stats(
        &self,
        params: Parameters<CfStatsParams>,
    ) -> Result<Json<StatsResponse>, String> {
        let engine = self.engine.clone();
        let p = params.0;
        spawn_blocking(move || engine.stats(p.include_tests, p.policy.unwrap_or_default()))
            .await
            .map_err(|e| format!("task join error: {e}"))?
            .map(Json)
            .map_err(|e| e.to_string())
    }

    #[tool(description = "List nodes with highest CF.")]
    async fn top_cf(&self, params: Parameters<TopParams>) -> Result<Json<TopResponse>, String> {
        let engine = self.engine.clone();
        let p = params.0;
        let node_type = p.node_type.unwrap_or_else(|| "all".to_string());
        spawn_blocking(move || {
            engine.top(
                p.limit.unwrap_or(10),
                &node_type,
                p.include_tests,
                p.policy.unwrap_or_default(),
            )
        })
        .await
        .map_err(|e| format!("task join error: {e}"))?
        .map(Json)
        .map_err(|e| e.to_string())
    }

    #[tool(description = "Search for symbols by keyword.")]
    async fn search_symbols(
        &self,
        params: Parameters<SearchParams>,
    ) -> Result<Json<SearchResponse>, String> {
        let engine = self.engine.clone();
        let p = params.0;
        spawn_blocking(move || {
            engine.search(
                &p.pattern,
                p.with_cf,
                p.limit,
                p.include_tests,
                p.policy.unwrap_or_default(),
            )
        })
        .await
        .map_err(|e| format!("task join error: {e}"))?
        .map(Json)
        .map_err(|e| e.to_string())
    }

    #[tool(
        description = "Get the context contributing to a symbol's CF (optionally include code)."
    )]
    async fn context(
        &self,
        params: Parameters<ContextRequest>,
    ) -> Result<Json<ContextResponse>, String> {
        let engine = self.engine.clone();
        let req = params.0;
        spawn_blocking(move || engine.context(req))
            .await
            .map_err(|e| format!("task join error: {e}"))?
            .map(Json)
            .map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CfStatsParams {
    #[serde(default)]
    pub include_tests: bool,
    pub policy: Option<PolicyKind>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct TopParams {
    pub limit: Option<usize>,
    pub node_type: Option<String>, // all|function|type|variable
    #[serde(default)]
    pub include_tests: bool,
    pub policy: Option<PolicyKind>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchParams {
    pub pattern: String,
    #[serde(default)]
    pub with_cf: bool,
    pub limit: Option<usize>,
    #[serde(default)]
    pub include_tests: bool,
    pub policy: Option<PolicyKind>,
}

#[tool_handler]
impl ServerHandler for CfMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Compute Context Footprint (CF) metrics from a pre-built SCIP context graph."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::edge::EdgeKind;
    use crate::domain::graph::ContextGraph;
    use crate::domain::node::{FunctionNode, Node, NodeCore, SourceSpan, Visibility};
    use crate::domain::ports::SourceReader;
    use std::path::Path;
    use std::sync::Arc;

    struct MockReader;
    impl SourceReader for MockReader {
        fn read(&self, _path: &Path) -> anyhow::Result<String> {
            Ok("line1\n".into())
        }
        fn read_lines(
            &self,
            _path: &str,
            _start_line: usize,
            _end_line: usize,
        ) -> anyhow::Result<Vec<String>> {
            Ok(vec!["line1".into()])
        }
    }

    fn make_graph() -> ContextGraph {
        let mut g = ContextGraph::new();
        let core = NodeCore::new(
            0,
            "f".into(),
            None,
            10,
            SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 0,
            },
            1.0,
            false,
            "app.py".into(),
        );
        let f = Node::Function(FunctionNode {
            core,
            parameters: Vec::new(),
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_types: vec![],
            throws: vec![],
        });
        let idx = g.add_node("sym/f().".into(), f);
        g.add_edge(idx, idx, EdgeKind::Call);
        g
    }

    #[tokio::test]
    async fn test_mcp_tools_smoke() {
        let engine = ContextEngine::from_prebuilt(
            "index.scip".into(),
            "/repo".into(),
            make_graph(),
            Arc::new(MockReader),
        );
        let server = CfMcpServer::new(engine);

        let compute = server
            .compute_cf(Parameters(ComputeRequest {
                symbols: vec!["sym/f().".into()],
                policy: PolicyKind::Academic,
                max_tokens: None,
            }))
            .await
            .unwrap()
            .0;
        assert_eq!(compute.starting_symbols, vec!["sym/f()."]);
        assert!(compute.total_context_size > 0);

        let _stats = server
            .cf_stats(Parameters(CfStatsParams {
                include_tests: true,
                policy: Some(PolicyKind::Academic),
            }))
            .await
            .unwrap()
            .0;

        let _top = server
            .top_cf(Parameters(TopParams {
                limit: Some(10),
                node_type: Some("all".into()),
                include_tests: true,
                policy: Some(PolicyKind::Academic),
            }))
            .await
            .unwrap()
            .0;

        let _search = server
            .search_symbols(Parameters(SearchParams {
                pattern: "sym".into(),
                with_cf: true,
                limit: None,
                include_tests: true,
                policy: Some(PolicyKind::Academic),
            }))
            .await
            .unwrap()
            .0;

        let _context = server
            .context(Parameters(ContextRequest {
                symbol: "sym/f().".into(),
                policy: PolicyKind::Academic,
                max_tokens: None,
                include_code: false,
            }))
            .await
            .unwrap()
            .0;
    }
}
