use super::TestDetector;

/// Go test code detector
///
/// Conventions:
/// - *_test.go files
pub struct GoTestDetector;

impl TestDetector for GoTestDetector {
    fn is_test_code(&self, _symbol: &str, file_path: &str) -> bool {
        // Go test files always end with _test.go
        file_path.ends_with("_test.go")
    }

    fn language(&self) -> &str {
        "go"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_go_test_file() {
        let detector = GoTestDetector;
        assert!(detector.is_test_code("", "pkg/foo_test.go"));
        assert!(detector.is_test_code("", "foo_test.go"));
        assert!(!detector.is_test_code("", "pkg/foo.go"));
        assert!(!detector.is_test_code("", "main.go"));
    }

    #[test]
    fn test_language_returns_go() {
        let detector = GoTestDetector;
        assert_eq!(detector.language(), "go");
    }
}
