//! Test code detection adapters
//!
//! Different languages have different conventions for test code.
//! This module provides language-specific test detection strategies.

mod go;
mod java;
mod javascript;
mod python;
mod rust;

pub use go::GoTestDetector;
pub use java::JavaTestDetector;
pub use javascript::JavaScriptTestDetector;
pub use python::PythonTestDetector;
pub use rust::RustTestDetector;

/// Trait for detecting test code based on language conventions
pub trait TestDetector: Send + Sync {
    /// Check if a symbol or file path indicates test code
    fn is_test_code(&self, symbol: &str, file_path: &str) -> bool;

    /// Get the language this detector is for
    fn language(&self) -> &str;
}

/// Multi-language test detector that routes to language-specific detectors
pub struct UniversalTestDetector {
    detectors: Vec<Box<dyn TestDetector>>,
}

impl UniversalTestDetector {
    pub fn new() -> Self {
        Self {
            detectors: vec![
                Box::new(PythonTestDetector),
                Box::new(RustTestDetector),
                Box::new(JavaScriptTestDetector),
                Box::new(JavaTestDetector),
                Box::new(GoTestDetector),
            ],
        }
    }

    /// Detect test code by trying all language detectors
    pub fn is_test_code(&self, symbol: &str, file_path: &str) -> bool {
        // Try to infer language from file extension
        if let Some(detector) = self.detect_language(file_path) {
            return detector.is_test_code(symbol, file_path);
        }

        // Fallback: check with all detectors
        self.detectors
            .iter()
            .any(|d| d.is_test_code(symbol, file_path))
    }

    fn detect_language(&self, file_path: &str) -> Option<&dyn TestDetector> {
        if file_path.ends_with(".py") {
            return Some(&PythonTestDetector as &dyn TestDetector);
        } else if file_path.ends_with(".rs") {
            return Some(&RustTestDetector as &dyn TestDetector);
        } else if file_path.ends_with(".js")
            || file_path.ends_with(".ts")
            || file_path.ends_with(".jsx")
            || file_path.ends_with(".tsx")
        {
            return Some(&JavaScriptTestDetector as &dyn TestDetector);
        } else if file_path.ends_with(".java") {
            return Some(&JavaTestDetector as &dyn TestDetector);
        } else if file_path.ends_with(".go") {
            return Some(&GoTestDetector as &dyn TestDetector);
        }
        None
    }
}

impl Default for UniversalTestDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_routes_by_extension() {
        let d = UniversalTestDetector::new();
        // Python
        assert!(d.is_test_code("", "tests/foo.py"));
        assert!(d.is_test_code("module/test_foo().", "src/module.py"));
        // Rust
        assert!(d.is_test_code("", "tests/bar.rs"));
        assert!(d.is_test_code("", "crate/bar_test.rs"));
        // JavaScript
        assert!(d.is_test_code("", "src/__tests__/api.test.js"));
        assert!(d.is_test_code("", "thing.spec.ts"));
        // Java
        assert!(d.is_test_code("", "src/test/java/ExampleTest.java"));
        // Go
        assert!(d.is_test_code("", "pkg/foo_test.go"));
    }

    #[test]
    fn test_universal_fallback_when_no_extension() {
        let d = UniversalTestDetector::new();
        // Unknown extension: fallback to "any detector matches"
        assert!(d.is_test_code("", "some/path/tests/script"));
        assert!(d.is_test_code("scip-python ... `m`/test_foo().", "noext"));
    }

    #[test]
    fn test_universal_default_constructs() {
        let d = UniversalTestDetector::default();
        assert!(!d.is_test_code("", "src/main.go"));
    }
}
