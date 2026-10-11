use super::*;

// ── Content ──────────────────────────────────────────────────────────────────

#[test]
fn content_wraps_content_lines() {
    // -- Setup & Fixtures
    let content = Content {
        lines: vec![ContentLine::single("x".to_string(), StyleHint::Normal)],
    };

    // -- Check
    assert_eq!(content.lines.len(), 1);
    assert_eq!(content.lines[0].spans[0].text, "x");
}

// ── span / content_line (pre-existing) ──────────────────────────────────────

#[test]
fn span_normal_constructor_sets_hint() {
    let span = Span::normal("x".to_string());
    assert_eq!(
        span,
        Span {
            text: "x".to_string(),
            hint: StyleHint::Normal
        }
    );
}

#[test]
fn span_new_stores_arbitrary_hint() {
    let span = Span::new("y".to_string(), StyleHint::DiffAdd);
    assert_eq!(span.text, "y");
    assert_eq!(span.hint, StyleHint::DiffAdd);
}

#[test]
fn content_line_single_creates_one_span() {
    let line = ContentLine::single("hello".to_string(), StyleHint::Emphasis);
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].text, "hello");
    assert_eq!(line.spans[0].hint, StyleHint::Emphasis);
}

#[test]
fn content_line_empty_has_no_spans() {
    assert!(ContentLine::empty().spans.is_empty());
}

#[test]
fn content_line_from_spans_preserves_order() {
    let spans = vec![
        Span::normal("a".to_string()),
        Span::meta("b".to_string()),
        Span::muted("c".to_string()),
    ];
    let line = ContentLine::from_spans(spans.clone());
    assert_eq!(line.spans, spans);
}

#[test]
fn content_line_single_defaults_diff_tint_none() {
    let line = ContentLine::single("hello".to_string(), StyleHint::Normal);
    assert_eq!(line.diff_tint, None);
}

#[test]
fn content_line_from_spans_defaults_diff_tint_none() {
    let line = ContentLine::from_spans(vec![Span::normal("a".to_string())]);
    assert_eq!(line.diff_tint, None);
}

#[test]
fn content_line_empty_defaults_diff_tint_none() {
    assert_eq!(ContentLine::empty().diff_tint, None);
}

#[test]
fn content_line_default_has_no_diff_tint() {
    let line = ContentLine::default();
    assert!(line.spans.is_empty());
    assert_eq!(line.hang_indent, 0);
    assert_eq!(line.diff_tint, None);
}

#[test]
fn content_line_single_with_tint_stores_tint() {
    let line =
        ContentLine::single_with_tint("+added".to_string(), StyleHint::MdCodePlain, DiffTint::Add);
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].text, "+added");
    assert_eq!(line.spans[0].hint, StyleHint::MdCodePlain);
    assert_eq!(line.diff_tint, Some(DiffTint::Add));
}

#[test]
fn diff_tint_variants_are_distinct() {
    assert_ne!(DiffTint::Add, DiffTint::Remove);
    assert_ne!(DiffTint::Add, DiffTint::Context);
    assert_ne!(DiffTint::Remove, DiffTint::Context);
}
