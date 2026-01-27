use super::TestDetector;

/// JavaScript/TypeScript test code detector
///
/// Conventions:
/// - *.test.js, *.test.ts files
/// - *.spec.js, *.spec.ts files
/// - __tests__/ directories
/// - tests/ directories
pub struct JavaScriptTestDetector;

impl TestDetector for JavaScriptTestDetector {
    fn is_test_code(&self, _symbol: &str, file_path: &str) -> bool {
        // Check test directories
        if file_path.contains("/__tests__/")
            || file_path.contains("/tests/")
            || file_path.starts_with("__tests__/")
            || file_path.starts_with("tests/")
        {
            return true;
        }

        // Check test file patterns
        if file_path.ends_with(".test.js")
            || file_path.ends_with(".test.ts")
            || file_path.ends_with(".test.jsx")
            || file_path.ends_with(".test.tsx")
            || file_path.ends_with(".spec.js")
            || file_path.ends_with(".spec.ts")
            || file_path.ends_with(".spec.jsx")
            || file_path.ends_with(".spec.tsx")
        {
            return true;
        }

        false
    }

    fn language(&self) -> &str {
        "javascript"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_js_test_directories() {
        let detector = JavaScriptTestDetector;
        assert!(detector.is_test_code("", "src/__tests__/api.test.js"));
        assert!(detector.is_test_code("", "app/tests/unit/foo.js"));
        assert!(detector.is_test_code("", "__tests__/bar.js"));
        assert!(detector.is_test_code("", "tests/helper.ts"));
        assert!(!detector.is_test_code("", "src/components/Button.tsx"));
    }

    #[test]
    fn test_detects_js_test_file_patterns() {
        let detector = JavaScriptTestDetector;
        assert!(detector.is_test_code("", "api.test.js"));
        assert!(detector.is_test_code("", "api.test.ts"));
        assert!(detector.is_test_code("", "api.test.jsx"));
        assert!(detector.is_test_code("", "api.test.tsx"));
        assert!(detector.is_test_code("", "api.spec.js"));
        assert!(detector.is_test_code("", "api.spec.ts"));
        assert!(detector.is_test_code("", "api.spec.jsx"));
        assert!(detector.is_test_code("", "api.spec.tsx"));
        assert!(!detector.is_test_code("", "src/api.js"));
        assert!(!detector.is_test_code("", "src/api.ts"));
    }

    #[test]
    fn test_language_returns_javascript() {
        let detector = JavaScriptTestDetector;
        assert_eq!(detector.language(), "javascript");
    }
}
