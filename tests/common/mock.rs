//! Mock implementations for integration tests.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use context_footprint::domain::policy::{DocumentationScorer, NodeInfo, SizeFunction, SourceSpan};
use context_footprint::domain::ports::SourceReader;

/// Mock SizeFunction that returns a fixed value.
pub struct MockSizeFunction {
    pub fixed_size: u32,
}

impl MockSizeFunction {
    pub fn new() -> Self {
        Self { fixed_size: 10 }
    }

    pub fn with_size(fixed_size: u32) -> Self {
        Self { fixed_size }
    }
}

impl Default for MockSizeFunction {
    fn default() -> Self {
        Self::new()
    }
}

impl SizeFunction for MockSizeFunction {
    fn compute(&self, _source: &str, _span: &SourceSpan, _doc_texts: &[String]) -> u32 {
        self.fixed_size
    }
}

/// Mock DocumentationScorer with configurable score.
pub struct MockDocScorer {
    pub default_score: f32,
}

impl MockDocScorer {
    pub fn new() -> Self {
        Self { default_score: 0.5 }
    }

    pub fn with_score(default_score: f32) -> Self {
        Self { default_score }
    }

    /// Returns 1.0 for docs, 0.0 for none (well-documented boundary).
    #[allow(dead_code)]
    pub fn strict() -> Self {
        Self { default_score: 1.0 }
    }
}

impl Default for MockDocScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentationScorer for MockDocScorer {
    fn score(&self, _node_info: &NodeInfo, doc_text: Option<&str>) -> f32 {
        if let Some(doc) = doc_text {
            if doc.trim().is_empty() {
                0.0
            } else {
                self.default_score
            }
        } else {
            0.0
        }
    }
}

/// Mock SourceReader that serves content from an in-memory map.
pub struct MockSourceReader {
    files: HashMap<PathBuf, String>,
}

impl MockSourceReader {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    pub fn with_file(mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Self {
        self.files
            .insert(path.as_ref().to_path_buf(), content.into());
        self
    }

    #[allow(dead_code)]
    pub fn add_file(&mut self, path: impl AsRef<Path>, content: impl Into<String>) {
        self.files
            .insert(path.as_ref().to_path_buf(), content.into());
    }
}

impl Default for MockSourceReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceReader for MockSourceReader {
    fn read(&self, path: &Path) -> Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow!("File not found: {}", path.display()))
    }

    fn read_lines(&self, path: &str, start_line: usize, end_line: usize) -> Result<Vec<String>> {
        let path_buf = PathBuf::from(path);
        let content = self
            .files
            .get(&path_buf)
            .ok_or_else(|| anyhow!("File not found: {}", path))?;

        let lines: Vec<String> = content.lines().map(String::from).collect();

        // Semantic data spans are 0-indexed.
        let start_idx = start_line;
        let end_idx = (end_line + 1).min(lines.len());

        if start_idx >= lines.len() {
            return Ok(Vec::new());
        }

        Ok(lines[start_idx..end_idx].to_vec())
    }
}
