use crate::domain::policy::{DocumentationScorer, NodeInfo};

/// Heuristic documentation scorer
/// Based on length and keywords (e.g., "Returns", "Args", "Raises")
pub struct HeuristicDocScorer;

impl Default for HeuristicDocScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl HeuristicDocScorer {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentationScorer for HeuristicDocScorer {
    fn score(&self, _node_info: &NodeInfo, doc_text: Option<&str>) -> f32 {
        let doc = match doc_text {
            Some(d) if !d.trim().is_empty() => d,
            _ => return 0.0,
        };

        let mut score: f32 = 0.0;

        // Length-based score (0-0.4)
        let word_count = doc.split_whitespace().count();
        if word_count > 50 {
            score += 0.4;
        } else if word_count > 20 {
            score += 0.3;
        } else if word_count > 10 {
            score += 0.2;
        } else if word_count > 5 {
            score += 0.1;
        }

        // Keyword-based score (0-0.6)
        let doc_lower = doc.to_lowercase();
        let mut keyword_score: f32 = 0.0;

        // Common documentation keywords
        if doc_lower.contains("returns") || doc_lower.contains("return") {
            keyword_score += 0.2;
        }
        if doc_lower.contains("args")
            || doc_lower.contains("parameters")
            || doc_lower.contains("param")
        {
            keyword_score += 0.2;
        }
        if doc_lower.contains("raises")
            || doc_lower.contains("exceptions")
            || doc_lower.contains("throws")
        {
            keyword_score += 0.1;
        }
        if doc_lower.contains("example") || doc_lower.contains("usage") {
            keyword_score += 0.1;
        }

        score += keyword_score.min(0.6);

        score.min(1.0)
    }
}
