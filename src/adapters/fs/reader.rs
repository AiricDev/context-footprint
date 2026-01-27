use crate::domain::ports::SourceReader;
use anyhow::{Context, Result};
use std::path::Path;

/// File system source reader implementation
pub struct FileSourceReader;

impl Default for FileSourceReader {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSourceReader {
    pub fn new() -> Self {
        Self
    }
}

impl SourceReader for FileSourceReader {
    fn read(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read source file: {}", path.display()))
    }
}
