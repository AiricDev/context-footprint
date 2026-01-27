// SCIP protocol parsing utilities
// This module contains helper functions for parsing SCIP symbols and ranges

// Helper functions for SCIP parsing

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
}
