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
        if let Some(filename) = file_path.split('/').last() {
            if filename.ends_with("_test.rs") {
                return true;
            }
        }

        // Note: #[test] annotations are not visible in SCIP symbols
        // We rely on file/directory structure

        false
    }

    fn language(&self) -> &str {
        "rust"
    }
}
