use crate::domain::policy::{DocumentationScorer, NodeInfo};

/// Simple documentation scorer
/// Binary scoring: has docstring = 1.0, no doc = 0.0
pub struct SimpleDocScorer;

impl SimpleDocScorer {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentationScorer for SimpleDocScorer {
    fn score(&self, _node_info: &NodeInfo, doc_text: Option<&str>) -> f32 {
        if let Some(doc) = doc_text {
            if !doc.trim().is_empty() {
                1.0
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
}
