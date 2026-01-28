use crate::domain::policy::{DocumentationScorer, NodeInfo};

/// Trait for language-specific documentation extraction logic
trait LanguageDocExtractor: Send + Sync {
    /// Extracts parameter names from a signature string
    fn extract_params(&self, signature: &str) -> Vec<String>;

    /// Does the signature indicate a return value?
    fn has_return_value(&self, signature: &str) -> bool;

    /// Checks if a parameter is mentioned in the documentation
    fn mentions_param(&self, doc_lower: &str, param_name: &str) -> bool {
        let param_lower = param_name.to_lowercase();
        // Check if the parameter name appears as a whole word
        doc_lower
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == param_lower)
    }

    /// Checks if return value is mentioned in the documentation
    fn mentions_return(&self, doc_lower: &str) -> bool {
        doc_lower.contains("return") || doc_lower.contains("returns") || doc_lower.contains("返回")
    }
}

struct GenericExtractor;
impl LanguageDocExtractor for GenericExtractor {
    fn extract_params(&self, _signature: &str) -> Vec<String> {
        vec![]
    }
    fn has_return_value(&self, _signature: &str) -> bool {
        false
    }
}

struct RustExtractor;
impl LanguageDocExtractor for RustExtractor {
    fn extract_params(&self, signature: &str) -> Vec<String> {
        // Simple regex-less extraction for Rust fn signature: fn name(a: T, b: T) -> R
        if let Some(start) = signature.find('(')
            && let Some(end) = signature.find(')')
        {
            let params_str = &signature[start + 1..end];
            return params_str
                .split(',')
                .filter_map(|p| {
                    let parts: Vec<&str> = p.split(':').collect();
                    parts.first().map(|s| s.trim().to_string())
                })
                .filter(|s| !s.is_empty() && s != "self" && s != "&self" && s != "&mut self")
                .collect();
        }
        vec![]
    }

    fn has_return_value(&self, signature: &str) -> bool {
        signature.contains("->") && !signature.contains("-> ()") && !signature.contains("-> !")
    }
}

struct PythonExtractor;
impl LanguageDocExtractor for PythonExtractor {
    fn extract_params(&self, signature: &str) -> Vec<String> {
        // def name(a: T, b=V) -> R
        if let Some(start) = signature.find('(')
            && let Some(end) = signature.rfind(')')
        {
            let params_str = &signature[start + 1..end];
            return params_str
                .split(',')
                .filter_map(|p| {
                    // Split by : (type hint), = (default value), or just take the name
                    let name_part = p.split(':').next()?.split('=').next()?.trim();
                    // Remove leading * or ** for star-args
                    let clean_name = name_part.trim_start_matches('*');
                    Some(clean_name.to_string())
                })
                .filter(|s| !s.is_empty() && s != "self" && s != "cls")
                .collect();
        }
        vec![]
    }

    fn has_return_value(&self, signature: &str) -> bool {
        signature.contains("->") && !signature.contains("-> None")
    }
}

/// Heuristic documentation scorer
/// Evaluates length, keyword presence, and coverage of parameters/return values.
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

    fn get_extractor(&self, language: Option<&str>) -> Box<dyn LanguageDocExtractor> {
        match language {
            Some("rs") | Some("rust") => Box::new(RustExtractor),
            Some("py") | Some("python") => Box::new(PythonExtractor),
            _ => Box::new(GenericExtractor),
        }
    }
}

impl DocumentationScorer for HeuristicDocScorer {
    fn score(&self, node_info: &NodeInfo, doc_text: Option<&str>) -> f32 {
        let doc = match doc_text {
            Some(d) if !d.trim().is_empty() => d,
            _ => return 0.0,
        };

        let mut score: f32 = 0.0;
        let doc_lower = doc.to_lowercase();
        let word_count = doc.split_whitespace().count();

        // 1. Description Quality (0.0 - 0.3)
        if word_count > 30 {
            score += 0.3;
        } else if word_count > 15 {
            score += 0.2;
        } else if word_count > 5 {
            score += 0.1;
        }

        // 2. Coverage (0.0 - 0.7)
        let mut coverage_score: f32 = 0.0;
        let extractor = self.get_extractor(node_info.language.as_deref());

        if let Some(signature) = &node_info.signature {
            let params = extractor.extract_params(signature);
            let has_ret = extractor.has_return_value(signature);

            let param_contribution = if !params.is_empty() {
                let covered_count = params
                    .iter()
                    .filter(|p| extractor.mentions_param(&doc_lower, p))
                    .count();
                (covered_count as f32 / params.len() as f32) * 0.4
            } else {
                // If no parameters, give a small bonus for non-empty doc
                0.2
            };

            let return_contribution = if has_ret {
                if extractor.mentions_return(&doc_lower) {
                    0.3
                } else {
                    0.0
                }
            } else {
                // If no return value, give a small bonus
                0.1
            };

            coverage_score = param_contribution + return_contribution;
        } else {
            // Fallback for no signature: keyword-based
            if doc_lower.contains("returns")
                || doc_lower.contains("return")
                || doc_lower.contains("返回")
            {
                coverage_score += 0.3;
            }
            if doc_lower.contains("args")
                || doc_lower.contains("parameters")
                || doc_lower.contains("param")
            {
                coverage_score += 0.3;
            }
            if doc_lower.contains("example") || doc_lower.contains("usage") {
                coverage_score += 0.1;
            }
        }

        score += coverage_score;
        score.min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::NodeType;

    fn node_info(language: Option<&str>, signature: Option<&str>) -> NodeInfo {
        NodeInfo {
            node_type: NodeType::Function,
            name: "test".into(),
            signature: signature.map(|s| s.to_string()),
            language: language.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_rust_param_coverage() {
        let s = HeuristicDocScorer::new();
        let info = node_info(Some("rs"), Some("fn foo(bar: i32, baz: String) -> bool"));

        // Doc mentions both params and return
        let doc = "Checks if bar and baz are valid. Returns true if so.";
        let score = s.score(&info, Some(doc));
        // Description: >5 words (0.1)
        // Coverage: params (2/2 * 0.4 = 0.4) + return (0.3) = 0.7
        // Total: 0.8
        assert!(score >= 0.8, "Score should be >= 0.8, got {}", score);

        // Doc mentions only one param
        let doc2 = "Checks if bar is valid.";
        let score2 = s.score(&info, Some(doc2));
        // Description: >4 words (0.1) -- wait, "Checks if bar is valid" is 5 words.
        // Word count is 5. matches >5 is false. So 0.0 for description if word count is 5.
        // Let's check: "Checks" "if" "bar" "is" "valid." -> 5 words.
        // Coverage: params (1/2 * 0.4 = 0.2) + return (0.0) = 0.2
        // Total: 0.2 or 0.3
        assert!(score2 < 0.5);
        assert!(score2 >= 0.2);
    }

    #[test]
    fn test_python_param_coverage() {
        let s = HeuristicDocScorer::new();
        let info = node_info(Some("py"), Some("def foo(bar: int, baz='hello') -> str:"));

        let doc = "Concatenates bar and baz. Returns a string.";
        let score = s.score(&info, Some(doc));
        assert!(score >= 0.7);
    }

    #[test]
    fn test_no_params_no_return() {
        let s = HeuristicDocScorer::new();
        let info = node_info(Some("rs"), Some("fn foo()"));

        let doc = "A simple function that does nothing.";
        let score = s.score(&info, Some(doc));
        // Description: >5 words (0.1)
        // Coverage: no params (0.2 bonus) + no return (0.1 bonus) = 0.3
        // Total: 0.4
        assert!((score - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_keyword_fallback() {
        let s = HeuristicDocScorer::new();
        let info = node_info(None, None); // No signature, no language

        let doc = "This is a function. Returns something. Args: none.";
        let score = s.score(&info, Some(doc));
        // Description: 9 words (0.1)
        // Coverage: returns (0.3) + args (0.3) = 0.6
        // Total: 0.7
        assert!((score - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_empty_doc_is_zero() {
        let s = HeuristicDocScorer::new();
        let info = node_info(None, None);
        assert_eq!(s.score(&info, None), 0.0);
        assert_eq!(s.score(&info, Some("  ")), 0.0);
    }

    #[test]
    fn test_partial_parameter_score() {
        let s = HeuristicDocScorer::new();
        // 两个参数：x 和 y
        let info = node_info(Some("rs"), Some("fn multi(x: i32, y: i32)"));

        // 只提到了 x
        let doc = "This function uses x but forgets the other.";
        let score = s.score(&info, Some(doc));

        // 预期评分逻辑：
        // Description: 8 words (>5) -> 0.1
        // Coverage:
        //   params: (1/2 * 0.4) = 0.2
        //   no-return bonus: 0.1
        // Total: 0.4
        assert!((score - 0.4).abs() < 0.001, "Expected 0.4, got {}", score);
    }

    #[test]
    fn test_word_boundary_matching() {
        let s = HeuristicDocScorer::new();
        let info = node_info(Some("rs"), Some("fn search(id: u32)"));

        // 文档中包含 "valid_id" 或 "identity"，但不包含独立的 "id"
        let doc_bad = "It checks valid_id and verifies identity.";
        let score_bad = s.score(&info, Some(doc_bad));

        // 不应该匹配到参数 'id'
        // Coverage: params 0.0 + no-return 0.1 = 0.1
        // Description: >5 words = 0.1
        // Total: 0.2
        assert!(score_bad < 0.3);

        // 正确提及了 id
        let doc_good = "It checks the id of the user.";
        let score_good = s.score(&info, Some(doc_good));
        assert!(score_good > score_bad);
    }

    #[test]
    fn test_ignore_self_in_signatures() {
        let s = HeuristicDocScorer::new();

        // Rust &self
        let info_rs = node_info(Some("rs"), Some("fn method(&self, data: String)"));
        let doc = "Processes the data.";
        let score_rs = s.score(&info_rs, Some(doc));
        // 参数应该只有 data，被提到了，所以满分覆盖参数 (0.4)
        assert!(score_rs >= 0.5);

        // Python self
        let info_py = node_info(Some("py"), Some("def method(self, data: str):"));
        let score_py = s.score(&info_py, Some(doc));
        assert!(score_py >= 0.5);
    }

    #[test]
    fn test_complex_python_args() {
        let s = HeuristicDocScorer::new();
        let info = node_info(Some("py"), Some("def foo(*args, **kwargs):"));

        let doc = "Passes args and kwargs to another function.";
        let score = s.score(&info, Some(doc));
        // stars should be stripped, matching 'args' and 'kwargs'
        assert!(score >= 0.4);
    }
}
