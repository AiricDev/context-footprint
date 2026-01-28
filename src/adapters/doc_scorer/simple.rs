use crate::domain::policy::{DocumentationScorer, NodeInfo};

/// Simple documentation scorer
/// Binary scoring: has docstring = 1.0, no doc = 0.0
pub struct SimpleDocScorer;

impl Default for SimpleDocScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleDocScorer {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentationScorer for SimpleDocScorer {
    fn score(&self, _node_info: &NodeInfo, doc_text: Option<&str>) -> f32 {
        if let Some(doc) = doc_text {
            if !doc.trim().is_empty() { 1.0 } else { 0.0 }
        } else {
            0.0
        }
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
            language: None,
        }
    }

    #[test]
    fn test_no_doc_returns_zero() {
        let s = SimpleDocScorer::new();
        let info = node_info();
        assert_eq!(s.score(&info, None), 0.0);
    }

    #[test]
    fn test_empty_doc_returns_zero() {
        let s = SimpleDocScorer::new();
        let info = node_info();
        assert_eq!(s.score(&info, Some("")), 0.0);
        assert_eq!(s.score(&info, Some("   \n\t  ")), 0.0);
    }

    #[test]
    fn test_valid_doc_returns_one() {
        let s = SimpleDocScorer::new();
        let info = node_info();
        assert_eq!(s.score(&info, Some("Does something.")), 1.0);
        assert_eq!(s.score(&info, Some("  doc  ")), 1.0);
    }
}
