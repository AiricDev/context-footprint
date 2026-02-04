use crate::app::dto::*;
use crate::app::engine::ContextEngine;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task::spawn_blocking;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct HttpState {
    pub engine: ContextEngine,
}

#[derive(Debug, Clone, Deserialize)]
struct StatsQuery {
    #[serde(default)]
    include_tests: bool,
    #[serde(default)]
    policy: Option<PolicyKind>,
}

#[derive(Debug, Clone, Deserialize)]
struct TopQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_node_type")]
    node_type: String,
    #[serde(default)]
    include_tests: bool,
    #[serde(default)]
    policy: Option<PolicyKind>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchQuery {
    pattern: String,
    #[serde(default)]
    with_cf: bool,
    limit: Option<usize>,
    #[serde(default)]
    include_tests: bool,
    #[serde(default)]
    policy: Option<PolicyKind>,
}

fn default_limit() -> usize {
    10
}

fn default_node_type() -> String {
    "all".to_string()
}

#[derive(Debug, Clone, serde::Serialize)]
struct ApiErrorBody {
    error: String,
}

fn api_error(status: StatusCode, msg: impl Into<String>) -> impl IntoResponse {
    (status, Json(ApiErrorBody { error: msg.into() }))
}

pub fn build_router(engine: ContextEngine) -> Router {
    let state = Arc::new(HttpState { engine });

    Router::new()
        .route("/health", get(health))
        .route("/compute", post(compute))
        .route("/stats", get(stats))
        .route("/top", get(top))
        .route("/search", get(search))
        .route("/context", post(context))
        .route("/reload", post(reload))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

pub async fn serve(engine: ContextEngine, addr: SocketAddr) -> Result<()> {
    let app = build_router(engine);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
    Json(state.engine.health())
}

async fn reload(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
    let engine = state.engine.clone();
    match spawn_blocking(move || engine.reload()).await {
        Ok(Ok(res)) => Json(res).into_response(),
        Ok(Err(e)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task join error: {e}"),
        )
        .into_response(),
    }
}

async fn compute(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<ComputeRequest>,
) -> impl IntoResponse {
    let engine = state.engine.clone();
    match spawn_blocking(move || engine.compute(req)).await {
        Ok(Ok(res)) => Json(res).into_response(),
        Ok(Err(e)) => api_error(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task join error: {e}"),
        )
        .into_response(),
    }
}

async fn context(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<ContextRequest>,
) -> impl IntoResponse {
    let engine = state.engine.clone();
    match spawn_blocking(move || engine.context(req)).await {
        Ok(Ok(res)) => Json(res).into_response(),
        Ok(Err(e)) => api_error(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task join error: {e}"),
        )
        .into_response(),
    }
}

async fn stats(
    State(state): State<Arc<HttpState>>,
    Query(q): Query<StatsQuery>,
) -> impl IntoResponse {
    let engine = state.engine.clone();
    let policy = q.policy.unwrap_or_default();
    match spawn_blocking(move || engine.stats(q.include_tests, policy)).await {
        Ok(Ok(res)) => Json(res).into_response(),
        Ok(Err(e)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task join error: {e}"),
        )
        .into_response(),
    }
}

async fn top(State(state): State<Arc<HttpState>>, Query(q): Query<TopQuery>) -> impl IntoResponse {
    let engine = state.engine.clone();
    let node_type = q.node_type.clone();
    let policy = q.policy.unwrap_or_default();

    match spawn_blocking(move || engine.top(q.limit, &node_type, q.include_tests, policy)).await {
        Ok(Ok(res)) => Json(res).into_response(),
        Ok(Err(e)) => api_error(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task join error: {e}"),
        )
        .into_response(),
    }
}

async fn search(
    State(state): State<Arc<HttpState>>,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    let engine = state.engine.clone();
    let policy = q.policy.unwrap_or_default();

    match spawn_blocking(move || {
        engine.search(&q.pattern, q.with_cf, q.limit, q.include_tests, policy)
    })
    .await
    {
        Ok(Ok(res)) => Json(res).into_response(),
        Ok(Err(e)) => api_error(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task join error: {e}"),
        )
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::engine::ContextEngine;
    use crate::domain::edge::EdgeKind;
    use crate::domain::graph::ContextGraph;
    use crate::domain::node::{FunctionNode, Node, NodeCore, SourceSpan, Visibility};
    use crate::domain::ports::SourceReader;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::path::Path;
    use tower::ServiceExt;

    struct MockReader;
    impl SourceReader for MockReader {
        fn read(&self, _path: &Path) -> anyhow::Result<String> {
            Ok("line1\nline2\n".into())
        }
        fn read_lines(
            &self,
            _path: &str,
            start_line: usize,
            end_line: usize,
        ) -> anyhow::Result<Vec<String>> {
            let lines = vec!["line1".to_string(), "line2".to_string()];
            let start = start_line.min(lines.len().saturating_sub(1));
            let end = end_line.min(lines.len().saturating_sub(1));
            Ok(lines[start..=end].to_vec())
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
        });
        let idx = g.add_node("sym/f().".into(), f);
        g.add_edge(idx, idx, EdgeKind::Call); // self-loop, harmless
        g
    }

    #[tokio::test]
    async fn test_http_health_and_compute() {
        let engine = ContextEngine::from_prebuilt(
            "index.scip".into(),
            "/repo".into(),
            make_graph(),
            Arc::new(MockReader),
        );
        let app = build_router(engine);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = serde_json::json!({
          "symbols": ["sym/f()."],
          "policy": "academic"
        });

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/compute")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
