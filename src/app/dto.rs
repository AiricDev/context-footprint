use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PolicyKind {
    #[default]
    Academic,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HealthResponse {
    pub semantic_path: String,
    pub project_root: String,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ComputeRequest {
    pub symbols: Vec<String>,
    #[serde(default)]
    pub policy: PolicyKind,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ComputeResponse {
    pub starting_symbols: Vec<String>,
    pub total_context_size: u32,
    pub reachable_node_count: usize,
    pub reachable_nodes_by_layer: Vec<Vec<ReachableNode>>,
    pub reachable_nodes_ordered: Vec<ReachableNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReachableNode {
    pub id: u32,
    pub symbol: String,
    pub node_type: String,
    pub context_size: u32,
    pub file_path: String,
    pub span: SpanDto,
    pub doc_score: f32,
    pub is_external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpanDto {
    /// 0-based, inclusive start line.
    pub start_line: u32,
    /// 0-based, inclusive start column.
    pub start_column: u32,
    /// 0-based, inclusive end line.
    pub end_line: u32,
    /// 0-based, inclusive end column.
    pub end_column: u32,

    /// 1-based, inclusive start line (convenience).
    pub start_line_1based: u32,
    /// 1-based, inclusive end line (convenience).
    pub end_line_1based: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatsResponse {
    pub functions: CfDistribution,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CfDistribution {
    pub count: usize,
    pub percentiles: Vec<PercentileValue>,
    pub average: u64,
    pub median: u32,
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PercentileValue {
    pub percentile: u32,
    pub tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TopResponse {
    pub items: Vec<TopItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TopItem {
    pub symbol: String,
    pub node_type: String,
    pub cf: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResponse {
    pub items: Vec<SearchItem>,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchItem {
    pub symbol: String,
    pub node_type: String,
    pub cf: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextRequest {
    pub symbol: String,
    #[serde(default)]
    pub policy: PolicyKind,
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub include_code: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextResponse {
    pub symbol: String,
    pub total_context_size: u32,
    pub reachable_node_count: usize,
    pub layers: Vec<ContextLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextLayer {
    pub depth: usize,
    pub files: Vec<ContextFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextFile {
    pub file_path: String,
    pub nodes: Vec<ContextNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextNode {
    pub id: u32,
    pub symbol: String,
    pub node_type: String,
    pub context_size: u32,
    pub span: SpanDto,
    pub doc_score: f32,
    pub is_external: bool,
    pub code: Option<Vec<CodeLine>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeLine {
    pub line_number: u32, // 1-based
    pub text: String,
}
