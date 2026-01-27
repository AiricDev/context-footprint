use super::TestDetector;

/// Python test code detector
///
/// Conventions:
/// - test_*.py files
/// - *_test.py files
/// - tests/ directories
/// - test_* functions
/// - Test* classes
pub struct PythonTestDetector;

impl TestDetector for PythonTestDetector {
    fn is_test_code(&self, symbol: &str, file_path: &str) -> bool {
        // Check file path patterns
        if file_path.contains("/tests/")
            || file_path.contains("/test/")
            || file_path.starts_with("tests/")
            || file_path.starts_with("test/")
        {
            return true;
        }

        // Check file name patterns
        if file_path
            .split('/')
            .next_back()
            .is_some_and(|filename| filename.starts_with("test_") || filename.ends_with("_test.py"))
        {
            return true;
        }

        // Check symbol patterns
        // Python test functions: test_*
        if symbol.contains("/test_") || symbol.contains("#test_") {
            return true;
        }

        // Python test classes: Test*
        if symbol.contains("/Test") {
            // More precise: check if it's a class definition
            if symbol.contains("#") {
                // This is a method, check if class name starts with Test
                if symbol.split('#').next().is_some_and(|class_part| {
                    class_part
                        .split('/')
                        .next_back()
                        .is_some_and(|class_name| class_name.starts_with("Test"))
                }) {
                    return true;
                }
            } else if symbol
                .split('/')
                .next_back()
                .is_some_and(|s| s.starts_with("Test"))
            {
                return true;
            }
        }

        false
    }

    fn language(&self) -> &str {
        "python"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_test_directory() {
        let detector = PythonTestDetector;
        assert!(detector.is_test_code("", "tests/test_api.py"));
        assert!(detector.is_test_code("", "myproject/tests/test_utils.py"));
        assert!(!detector.is_test_code("", "src/utils.py"));
    }

    #[test]
    fn test_detects_test_file_prefix() {
        let detector = PythonTestDetector;
        assert!(detector.is_test_code("", "test_api.py"));
        assert!(detector.is_test_code("", "src/test_utils.py"));
        assert!(!detector.is_test_code("", "src/api.py"));
    }

    #[test]
    fn test_detects_test_file_suffix() {
        let detector = PythonTestDetector;
        assert!(detector.is_test_code("", "api_test.py"));
        assert!(detector.is_test_code("", "src/utils_test.py"));
        assert!(!detector.is_test_code("", "src/api.py"));
    }

    #[test]
    fn test_detects_test_function() {
        let detector = PythonTestDetector;
        assert!(detector.is_test_code(
            "scip-python python myapp ... `module`/test_my_function().",
            "src/module.py"
        ));
        assert!(!detector.is_test_code(
            "scip-python python myapp ... `module`/my_function().",
            "src/module.py"
        ));
    }

    #[test]
    fn test_detects_test_class() {
        let detector = PythonTestDetector;
        assert!(detector.is_test_code(
            "scip-python python myapp ... `module`/TestMyClass#",
            "src/module.py"
        ));
        assert!(detector.is_test_code(
            "scip-python python myapp ... `module`/TestMyClass#test_method().",
            "src/module.py"
        ));
        assert!(!detector.is_test_code(
            "scip-python python myapp ... `module`/MyClass#",
            "src/module.py"
        ));
    }
}
