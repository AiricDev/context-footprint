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

        // Check symbol patterns (semantic data format: module.name#Kind, module.Class#Type, module.Class.method#Function)
        let (name_part, kind_suffix) = match symbol.split_once('#') {
            Some((n, k)) => (n, k),
            None => return false,
        };
        let segments: Vec<&str> = name_part.split('.').collect();

        // Python test functions: name segment starts with test_
        if segments.last().is_some_and(|s| s.starts_with("test_")) {
            return true;
        }

        // Python test classes: Type symbol whose name segment starts with Test
        if kind_suffix == "Type" && segments.last().is_some_and(|s| s.starts_with("Test")) {
            return true;
        }

        // Python test methods: Function symbol with a segment starting with Test (method of Test* class)
        if kind_suffix == "Function" && segments.iter().any(|s| s.starts_with("Test")) {
            return true;
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
        // Semantic data format: module.name#Function
        assert!(detector.is_test_code("module.test_my_function#Function", "src/module.py"));
        assert!(!detector.is_test_code("module.my_function#Function", "src/module.py"));
    }

    #[test]
    fn test_detects_test_class() {
        let detector = PythonTestDetector;
        // Semantic data format: module.Class#Type, module.Class.method#Function
        assert!(detector.is_test_code("module.TestMyClass#Type", "src/module.py"));
        assert!(detector.is_test_code("module.TestMyClass.test_method#Function", "src/module.py"));
        assert!(!detector.is_test_code("module.MyClass#Type", "src/module.py"));
    }
}
