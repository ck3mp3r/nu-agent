//! The picker's match: a case-insensitive subsequence test over a row's text.

/// Subsequence match: every char of `query` appears in `candidate` in order.
/// Case-insensitive by contract — callers lowercase both sides.
pub(crate) fn fuzzy_matches(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut query_chars = query.chars();
    let mut needle = query_chars.next();
    for ch in candidate.chars() {
        if Some(ch) == needle {
            needle = query_chars.next();
            if needle.is_none() {
                return true;
            }
        }
    }
    false
}
