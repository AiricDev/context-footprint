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
        if file_path.split('/').next_back().is_some_and(|filename| {
            filename.ends_with("Test.java") || filename.ends_with("Tests.java")
        }) {
            return true;
        }

        false
    }

    fn language(&self) -> &str {
        "java"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_java_test_directories() {
        let detector = JavaTestDetector;
        assert!(detector.is_test_code("", "src/test/java/com/ExampleTest.java"));
        assert!(detector.is_test_code("", "project/test/MyTest.java"));
        assert!(detector.is_test_code("", "test/MyTest.java"));
        assert!(!detector.is_test_code("", "src/main/java/com/Example.java"));
    }

    #[test]
    fn test_detects_java_test_file_suffix() {
        let detector = JavaTestDetector;
        assert!(detector.is_test_code("", "com/example/ServiceTest.java"));
        assert!(detector.is_test_code("", "com/example/ServiceTests.java"));
        assert!(!detector.is_test_code("", "com/example/Service.java"));
    }

    #[test]
    fn test_language_returns_java() {
        let detector = JavaTestDetector;
        assert_eq!(detector.language(), "java");
    }
}
