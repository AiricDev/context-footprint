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

        // SCIP ranges are 0-indexed. start_line is inclusive, end_line is exclusive.
        let start_idx = start_line;
        let end_idx = (end_line + 1).min(lines.len()); // Make it inclusive for display if needed, but SCIP enclosing_range is usually inclusive

        if start_idx >= lines.len() {
            return Ok(Vec::new());
        }

        Ok(lines[start_idx..end_idx].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_file_source_reader_read() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        let content = "Hello, world!";

        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let reader = FileSourceReader::new();
        let read_content = reader.read(&file_path).unwrap();

        assert_eq!(read_content, content);
    }

    #[test]
    fn test_file_source_reader_read_lines() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lines.txt");
        let content = "line1\nline2\nline3\nline4\nline5";

        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let reader = FileSourceReader::new();
        let path_str = file_path.to_str().unwrap();

        // Test mid-range
        let lines = reader.read_lines(path_str, 1, 2).unwrap();
        assert_eq!(lines, vec!["line2", "line3"]);

        // Test out of bounds
        let lines = reader.read_lines(path_str, 10, 15).unwrap();
        assert!(lines.is_empty());

        // Test boundary
        let lines = reader.read_lines(path_str, 0, 0).unwrap();
        assert_eq!(lines, vec!["line1"]);

        let lines = reader.read_lines(path_str, 4, 10).unwrap();
        assert_eq!(lines, vec!["line5"]);
    }

    #[test]
    fn test_file_source_reader_read_nonexistent() {
        let reader = FileSourceReader::new();
        let result = reader.read(Path::new("nonexistent_file_12345.txt"));
        assert!(result.is_err());
    }
}
