// ── Helpers ──────────────────────────────────────────────────────────────────

pub(super) fn test_line_count(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }

    content.split_inclusive('\n').count()
}
