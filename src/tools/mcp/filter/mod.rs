/// Returns true if `tool_name` matches at least one provided glob pattern.
///
/// Behavior:
/// - Empty patterns => match all tools
/// - `*` matches zero or more characters
/// - Matching is case-sensitive
pub fn matches_patterns(tool_name: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }

    patterns
        .iter()
        .any(|pattern| glob_match_case_sensitive(pattern, tool_name))
}

fn glob_match_case_sensitive(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();

    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_idx: Option<usize> = None;
    let mut match_idx = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star_idx = Some(pi);
            match_idx = ti;
            pi += 1;
        } else if let Some(star) = star_idx {
            pi = star + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }

    pi == p.len()
}

#[cfg(test)]
mod test;
