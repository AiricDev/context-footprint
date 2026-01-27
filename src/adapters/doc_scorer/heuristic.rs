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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::NodeType;

    fn node_info() -> NodeInfo {
        NodeInfo {
            node_type: NodeType::Function,
            name: "test".into(),
            signature: None,
        }
    }

    #[test]
    fn test_no_doc_returns_zero() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        assert_eq!(s.score(&info, None), 0.0);
    }

    #[test]
    fn test_empty_doc_returns_zero() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        assert_eq!(s.score(&info, Some("")), 0.0);
        assert_eq!(s.score(&info, Some("   \n\t  ")), 0.0);
    }

    #[test]
    fn test_short_doc_low_score() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        // < 5 words: no length score, no keywords
        assert_eq!(s.score(&info, Some("one two three")), 0.0);
        assert_eq!(s.score(&info, Some("a b c d")), 0.0);
    }

    #[test]
    fn test_medium_doc_with_keywords() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        // > 10 words -> 0.2 length; "returns" 0.2 + "args" 0.2 = 0.4 keyword; total 0.6
        let doc = "This function does something useful. Returns the result. Args: x, y.";
        assert!(
            (s.score(&info, Some(doc)) - 0.6).abs() < 0.001,
            "expected ~0.6, got {}",
            s.score(&info, Some(doc))
        );
    }

    #[test]
    fn test_long_doc_max_score() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        // > 50 words -> 0.4 length; returns + args + raises + example = 0.6 keyword; total 1.0
        let words: Vec<String> = (0..55).map(|i| format!("word{i}")).collect();
        let doc = format!(
            "{} Returns value. Args: a,b. Raises Error. Example usage here.",
            words.join(" ")
        );
        assert!(
            (s.score(&info, Some(&doc)) - 1.0).abs() < 0.001,
            "expected 1.0, got {}",
            s.score(&info, Some(&doc))
        );
    }

    #[test]
    fn test_keyword_score_capped_at_0_6() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        // <= 5 words so no length score; many keywords would sum > 0.6 but cap at 0.6
        let doc = "Returns args raises example";
        let score = s.score(&info, Some(doc));
        assert!(
            (score - 0.6).abs() < 0.001,
            "keyword contribution should be capped at 0.6, got {}",
            score
        );
    }

    #[test]
    fn test_total_score_capped_at_1_0() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        // Length tiers: >50 -> 0.4. With 0.6 from keywords, total = 1.0 cap
        let words: Vec<String> = (0..60).map(|i| format!("w{i}")).collect();
        let doc = format!("{} Returns. Args. Raises. Example.", words.join(" "));
        let score = s.score(&info, Some(&doc));
        assert!(
            (score - 1.0).abs() < 0.001,
            "total should be capped at 1.0, got {}",
            score
        );
    }

    #[test]
    fn test_length_tiers() {
        let s = HeuristicDocScorer::new();
        let info = node_info();
        // >5 words -> 0.1
        let doc6 = "a b c d e f";
        assert!((s.score(&info, Some(doc6)) - 0.1).abs() < 0.001);
        // >10 words -> 0.2
        let doc11 = "a b c d e f g h i j k";
        assert!((s.score(&info, Some(doc11)) - 0.2).abs() < 0.001);
        // >20 words -> 0.3
        let doc21: String = (0..21)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!((s.score(&info, Some(&doc21)) - 0.3).abs() < 0.001);
        // >50 words -> 0.4
        let doc51: String = (0..51)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!((s.score(&info, Some(&doc51)) - 0.4).abs() < 0.001);
    }
}
