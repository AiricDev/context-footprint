use crate::domain::policy::{SizeFunction, SourceSpan};

/// Tiktoken-based size function
/// Uses tiktoken to count tokens in the source code span
pub struct TiktokenSizeFunction;

impl TiktokenSizeFunction {
    pub fn new() -> Self {
        Self
    }
}

impl SizeFunction for TiktokenSizeFunction {
    fn compute(&self, source: &str, span: &SourceSpan) -> u32 {
        // Extract the code snippet from the span
        let lines: Vec<&str> = source.lines().collect();
        
        if span.start_line as usize >= lines.len() {
            return 0;
        }
        
        let start_line_idx = span.start_line as usize;
        let end_line_idx = (span.end_line as usize).min(lines.len() - 1);
        
        let mut code_snippet = String::new();
        
        if start_line_idx == end_line_idx {
            // Single line
            let line = lines[start_line_idx];
            let start_col = span.start_column as usize;
            let end_col = (span.end_column as usize).min(line.len());
            if start_col < line.len() {
                code_snippet.push_str(&line[start_col..end_col]);
            }
        } else {
            // Multiple lines
            // First line
            let first_line = lines[start_line_idx];
            let start_col = span.start_column as usize;
            if start_col < first_line.len() {
                code_snippet.push_str(&first_line[start_col..]);
            }
            code_snippet.push('\n');
            
            // Middle lines
            for i in (start_line_idx + 1)..end_line_idx {
                code_snippet.push_str(lines[i]);
                code_snippet.push('\n');
            }
            
            // Last line
            if end_line_idx < lines.len() {
                let last_line = lines[end_line_idx];
                let end_col = (span.end_column as usize).min(last_line.len());
                code_snippet.push_str(&last_line[..end_col]);
            }
        }
        
        // Use a simple token counting approach (approximate)
        // In a real implementation, you would use the tiktoken crate
        // For now, we'll use a simple word-based approximation
        count_tokens_approx(&code_snippet)
    }
}

fn count_tokens_approx(text: &str) -> u32 {
    // Simple approximation: count words and punctuation
    // In production, use tiktoken-rs crate
    text.split_whitespace()
        .map(|word| {
            // Rough approximation: 1 token per word, plus punctuation
            let punct_count = word.chars().filter(|c| !c.is_alphanumeric()).count();
            (1 + punct_count / 2).max(1)
        })
        .sum::<usize>() as u32
}
