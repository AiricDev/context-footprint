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

    if o_start_line < i_start_line || (o_start_line == i_start_line && o_start_char <= i_start_char) {
        if o_end_line > i_end_line || (o_end_line == i_end_line && o_end_char >= i_end_char) {
            return true;
        }
    }
    false
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
