use crate::domain::semantic::SemanticData;
use anyhow::Result;
use std::path::Path;

/// Semantic data source port (implemented by Infrastructure)
pub trait SemanticDataSource {
    fn load(&self) -> Result<SemanticData>;
}

/// Source code reader port
pub trait SourceReader: Send + Sync {
    fn read(&self, path: &Path) -> Result<String>;

    /// Read specific lines from a file (1-indexed, inclusive)
    fn read_lines(&self, path: &str, start_line: usize, end_line: usize) -> Result<Vec<String>>;
}
