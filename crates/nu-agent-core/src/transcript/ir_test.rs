use super::ir::*;

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
fn render_block_stores_role_and_lines() {
    let block = RenderBlock {
        role: Role::Tool,
        lines: vec![ContentLine::single("test".to_string(), StyleHint::Normal)],
    };
    assert_eq!(block.role, Role::Tool);
    assert_eq!(block.lines.len(), 1);
}

#[test]
fn display_line_new_stores_fields() {
    let dl = DisplayLine::new("foo".to_string(), StyleHint::Error);
    assert_eq!(dl.text, "foo");
    assert_eq!(dl.hint, StyleHint::Error);
}
