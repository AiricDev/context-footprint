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
