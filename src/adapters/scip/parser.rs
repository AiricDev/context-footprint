// SCIP protocol parsing utilities
// This module contains helper functions for parsing SCIP symbols and ranges

use regex::Regex;
use std::sync::OnceLock;

/// Parameter symbol ends with `.(name)`; this matches that suffix and captures the name.
/// Handles both method form `foo().(x)` and term form `foo.(x)`.
fn parameter_suffix_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\.\(([^)]*)\)$").expect("parameter suffix regex"))
}

/// Parse SCIP range to SourceRange
pub fn parse_range(range: &[i32]) -> (i32, i32, i32, i32) {
    if range.len() == 3 {
        (range[0], range[1], range[0], range[2])
    } else if range.len() == 4 {
        (range[0], range[1], range[2], range[3])
    } else {
        (0, 0, 0, 0)
    }
}

/// Check if inner range is enclosed by outer range
pub fn encloses(outer: &[i32], inner: &[i32]) -> bool {
    let (o_start_line, o_start_char, o_end_line, o_end_char) = parse_range(outer);
    let (i_start_line, i_start_char, i_end_line, i_end_char) = parse_range(inner);

    (o_start_line < i_start_line || (o_start_line == i_start_line && o_start_char <= i_start_char))
        && (o_end_line > i_end_line || (o_end_line == i_end_line && o_end_char >= i_end_char))
}

/// Extract the parent function/method symbol from a parameter symbol by parsing the SCIP symbol string.
///
/// In SCIP, a parameter descriptor is `(name)` and is appended to the function symbol:
/// `<function_symbol>.(param_name)`. We strip the trailing `.(name)` to get the function.
pub fn parent_function_from_parameter_symbol(symbol: &str) -> Option<String> {
    let re = parameter_suffix_regex();
    let m = re.find(symbol)?;
    // Include the '.' so parent is the full function symbol (e.g. "foo()." not "foo()")
    Some(symbol[..=m.start()].to_string())
}

/// Extract the parameter name from a parameter symbol (the part inside the last `.(...)`).
/// e.g. `pkg . foo().(x)` → `x`, `pkg . foo().(self)` → `self`.
pub fn parameter_name_from_symbol(symbol: &str) -> Option<String> {
    let re = parameter_suffix_regex();
    let cap = re.captures(symbol)?;
    cap.get(1).map(|m| m.as_str().to_string())
}

/// Find the smallest enclosing definition for a reference range
pub fn find_enclosing_definition<'a>(
    ref_range: &[i32],
    defs: &[(Vec<i32>, &'a str)],
) -> Option<&'a str> {
    let mut best_def: Option<&str> = None;
    let mut best_range: Option<&[i32]> = None;

    for (def_range, symbol) in defs {
        if encloses(def_range, ref_range) {
            if let Some(prev_range) = best_range {
                // We want the SMALLEST enclosing range
                if encloses(prev_range, def_range) {
                    best_def = Some(symbol);
                    best_range = Some(def_range);
                }
            } else {
                best_def = Some(symbol);
                best_range = Some(def_range);
            }
        }
    }
    best_def
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range() {
        assert_eq!(parse_range(&[1, 2, 3]), (1, 2, 1, 3));
        assert_eq!(parse_range(&[1, 2, 3, 4]), (1, 2, 3, 4));
        assert_eq!(parse_range(&[1]), (0, 0, 0, 0));
    }

    #[test]
    fn test_encloses() {
        let outer = vec![1, 0, 10, 0];
        let inner = vec![2, 0, 5, 0];
        let outside = vec![0, 0, 5, 0];

        assert!(encloses(&outer, &inner));
        assert!(!encloses(&inner, &outer));
        assert!(!encloses(&outer, &outside));

        // Same range
        assert!(encloses(&outer, &outer));
    }

    #[test]
    fn test_find_enclosing_definition() {
        let defs = vec![
            (vec![0, 0, 100, 0], "global"),
            (vec![10, 0, 50, 0], "func"),
            (vec![20, 0, 30, 0], "inner"),
        ];

        let ref_range = vec![25, 0, 26, 0];
        assert_eq!(find_enclosing_definition(&ref_range, &defs), Some("inner"));

        let ref_range_func = vec![15, 0, 16, 0];
        assert_eq!(
            find_enclosing_definition(&ref_range_func, &defs),
            Some("func")
        );

        let ref_range_none = vec![101, 0, 102, 0];
        assert_eq!(find_enclosing_definition(&ref_range_none, &defs), None);
    }

    #[test]
    fn test_parent_function_from_parameter_symbol() {
        // Parameter symbol: function symbol + ".(param_name)"
        assert_eq!(
            parent_function_from_parameter_symbol("scip python pkg . main . foo().(x)"),
            Some("scip python pkg . main . foo().".to_string())
        );
        assert_eq!(
            parent_function_from_parameter_symbol("pkg . Class.method().(self)"),
            Some("pkg . Class.method().".to_string())
        );
        // Real SCIP-style symbols (e.g. from scip-python with backtick package path)
        let method_param_self = "scip-python python airelay 0.1.0 `app.adapters.billing.arq_billing_adapter`/ArqBillingAdapter#emit_usage().(self)";
        assert_eq!(
            parent_function_from_parameter_symbol(method_param_self),
            Some("scip-python python airelay 0.1.0 `app.adapters.billing.arq_billing_adapter`/ArqBillingAdapter#emit_usage().".to_string())
        );
        let method_param_event = "scip-python python airelay 0.1.0 `app.adapters.billing.arq_billing_adapter`/ArqBillingAdapter#emit_usage().(event)";
        assert_eq!(
            parent_function_from_parameter_symbol(method_param_event),
            Some("scip-python python airelay 0.1.0 `app.adapters.billing.arq_billing_adapter`/ArqBillingAdapter#emit_usage().".to_string())
        );
        assert_eq!(parent_function_from_parameter_symbol("not_a_param"), None);
        assert_eq!(parent_function_from_parameter_symbol("foo()."), None);
        // Term form (function as term, not method)
        assert_eq!(
            parent_function_from_parameter_symbol("pkg . foo.(x)"),
            Some("pkg . foo.".to_string())
        );
    }

    #[test]
    fn test_parameter_name_from_symbol() {
        assert_eq!(
            parameter_name_from_symbol("foo().(x)"),
            Some("x".to_string())
        );
        assert_eq!(
            parameter_name_from_symbol("pkg . Class.method().(self)"),
            Some("self".to_string())
        );
        assert_eq!(
            parameter_name_from_symbol("scip-python python pkg . main . foo().(x)"),
            Some("x".to_string())
        );
        assert_eq!(
            parameter_name_from_symbol("scip-python python airelay 0.1.0 `app.adapters.billing.arq_billing_adapter`/ArqBillingAdapter#emit_usage().(self)"),
            Some("self".to_string())
        );
        assert_eq!(
            parameter_name_from_symbol("scip-python python airelay 0.1.0 `app.adapters.billing.arq_billing_adapter`/ArqBillingAdapter#emit_usage().(event)"),
            Some("event".to_string())
        );
        assert_eq!(parameter_name_from_symbol("foo()."), None);
    }
}
