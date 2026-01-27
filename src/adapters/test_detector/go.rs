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
