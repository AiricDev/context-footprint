use super::TestDetector;

/// Java test code detector
///
/// Conventions:
/// - *Test.java files
/// - src/test/ directories
/// - test/ directories
pub struct JavaTestDetector;

impl TestDetector for JavaTestDetector {
    fn is_test_code(&self, _symbol: &str, file_path: &str) -> bool {
        // Check test directories
        if file_path.contains("/src/test/")
            || file_path.contains("/test/")
            || file_path.starts_with("test/")
        {
            return true;
        }

        // Check test file suffix
        if let Some(filename) = file_path.split('/').last() {
            if filename.ends_with("Test.java") || filename.ends_with("Tests.java") {
                return true;
            }
        }

        false
    }

    fn language(&self) -> &str {
        "java"
    }
}
