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

    fn read_lines(&self, path: &str, start_line: usize, end_line: usize) -> Result<Vec<String>> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read source file: {}", path))?;
        
        let lines: Vec<String> = content.lines().map(String::from).collect();
        
        // Convert 1-indexed to 0-indexed and ensure valid range
        let start_idx = start_line.saturating_sub(1);
        let end_idx = end_line.min(lines.len());
        
        if start_idx >= lines.len() {
            return Ok(Vec::new());
        }
        
        Ok(lines[start_idx..end_idx].to_vec())
    }
}
