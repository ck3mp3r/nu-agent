use similar::{Algorithm, ChangeTag, TextDiff};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffBounds {
    pub max_bytes: usize,
    pub max_lines: usize,
}

impl Default for DiffBounds {
    fn default() -> Self {
        Self {
            max_bytes: 128 * 1024,
            max_lines: 4_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStats {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedDiff {
    pub text: String,
    pub truncated: bool,
    pub omitted_files: usize,
    pub omitted_hunks: usize,
    pub stats: DiffStats,
}

pub fn compute_edit_unified_diff(path: &Path, previous: &str, next: &str) -> UnifiedDiff {
    compute_edit_unified_diff_bounded(path, previous, next, DiffBounds::default())
}

pub fn compute_edit_unified_diff_bounded(
    path: &Path,
    previous: &str,
    next: &str,
    bounds: DiffBounds,
) -> UnifiedDiff {
    if previous == next {
        return UnifiedDiff {
            text: String::new(),
            truncated: false,
            omitted_files: 0,
            omitted_hunks: 0,
            stats: DiffStats {
                files_changed: 0,
                insertions: 0,
                deletions: 0,
            },
        };
    }

    let old_header = format!("a/{}", normalize_path_for_diff(path));
    let new_header = format!("b/{}", normalize_path_for_diff(path));
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .diff_lines(previous, next);

    let insertions = diff
        .iter_all_changes()
        .filter(|change| matches!(change.tag(), ChangeTag::Insert))
        .count();
    let deletions = diff
        .iter_all_changes()
        .filter(|change| matches!(change.tag(), ChangeTag::Delete))
        .count();

    let rendered = diff
        .unified_diff()
        .context_radius(3)
        .header(old_header.as_str(), new_header.as_str())
        .to_string();
    let (bounded_text, truncated, omitted_hunks) = apply_bounds(&rendered, bounds);

    UnifiedDiff {
        text: bounded_text,
        truncated,
        omitted_files: 0,
        omitted_hunks,
        stats: DiffStats {
            files_changed: 1,
            insertions,
            deletions,
        },
    }
}

fn normalize_path_for_diff(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.is_empty() {
        "file".to_string()
    } else {
        normalized
    }
}

fn split_lines_preserving_terminators(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }

    content.split_inclusive('\n').collect::<Vec<_>>()
}

fn apply_bounds(diff_text: &str, bounds: DiffBounds) -> (String, bool, usize) {
    let lines = split_lines_preserving_terminators(diff_text);
    let max_lines = bounds.max_lines.max(1);
    let max_bytes = bounds.max_bytes;

    let mut current_bytes = 0usize;
    let mut keep = 0usize;
    while keep < lines.len() {
        let line = lines[keep];
        if keep + 1 > max_lines || current_bytes + line.len() > max_bytes {
            break;
        }
        current_bytes += line.len();
        keep += 1;
    }

    if keep == lines.len() {
        return (diff_text.to_string(), false, 0);
    }

    let marker = loop {
        let omitted_hunks = count_hunks(&lines[keep..]);
        let marker_budget = max_bytes.saturating_sub(current_bytes);
        let marker =
            build_truncation_marker_bounded(lines.len() - keep, omitted_hunks, marker_budget);
        let marker_lines = usize::from(!marker.is_empty());

        if keep + marker_lines <= max_lines && current_bytes + marker.len() <= max_bytes {
            break marker;
        }

        if keep == 0 {
            break marker;
        }

        keep -= 1;
        current_bytes = current_bytes.saturating_sub(lines[keep].len());
    };

    let omitted_hunks = count_hunks(&lines[keep..]);
    let mut text = lines[..keep].concat();
    text.push_str(&marker);
    debug_assert!(text.len() <= max_bytes);
    (text, true, omitted_hunks)
}

fn count_hunks(lines: &[&str]) -> usize {
    lines.iter().filter(|line| line.starts_with("@@ ")).count()
}

fn build_truncation_marker(omitted_lines: usize, omitted_hunks: usize) -> String {
    format!("... diff truncated ... omitted_lines={omitted_lines} omitted_hunks={omitted_hunks}\n")
}

fn build_truncation_marker_bounded(
    omitted_lines: usize,
    omitted_hunks: usize,
    max_bytes: usize,
) -> String {
    let marker = build_truncation_marker(omitted_lines, omitted_hunks);
    if marker.len() <= max_bytes {
        return marker;
    }

    for compact in ["...\n", "...", "..", ".", ""] {
        if compact.len() <= max_bytes {
            return compact.to_string();
        }
    }

    String::new()
}
