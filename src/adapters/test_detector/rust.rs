use super::TestDetector;

/// Rust test code detector
///
/// Conventions:
/// - tests/ directories
/// - #[test] and #[cfg(test)] modules (detected in symbols)
/// - *_test.rs files
/// - Files in tests/ directory
pub struct RustTestDetector;

impl TestDetector for RustTestDetector {
    fn is_test_code(&self, _symbol: &str, file_path: &str) -> bool {
        // Check tests directory
        if file_path.contains("/tests/") || file_path.starts_with("tests/") {
            return true;
        }

        // Check test file suffix
        if file_path
            .split('/')
            .next_back()
            .is_some_and(|filename| filename.ends_with("_test.rs"))
        {
            return true;
        }

        // Note: #[test] annotations may not be visible in semantic symbol data
        // We rely on file/directory structure

        false
    }

    fn language(&self) -> &str {
        "rust"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_rust_tests_directory() {
        let detector = RustTestDetector;
        assert!(detector.is_test_code("", "tests/integration_test.rs"));
        assert!(detector.is_test_code("", "crate/tests/foo.rs"));
        assert!(detector.is_test_code("", "tests/helpers/mod.rs"));
        assert!(!detector.is_test_code("", "src/lib.rs"));
    }

    #[test]
    fn test_detects_rust_test_file_suffix() {
        let detector = RustTestDetector;
        assert!(detector.is_test_code("", "src/foo_test.rs"));
        assert!(detector.is_test_code("", "crate/bar_test.rs"));
        assert!(!detector.is_test_code("", "src/foo.rs"));
        assert!(!detector.is_test_code("", "src/lib.rs"));
    }

    #[test]
    fn test_language_returns_rust() {
        let detector = RustTestDetector;
        assert_eq!(detector.language(), "rust");
    }
}
