use super::*;

// ── Span / ContentLine ──────────────────────────────────────────────────────

/// Constructors: `Span` constructors set the hint, `ContentLine` constructors
/// build spans, and every constructor defaults `diff_tint` to `None` and
/// `hang_indent` to 0.
#[test]
fn content_line_constructors_and_tint_table() {
    // -- Setup & Fixtures
    let spans = vec![
        Span::normal("a".to_string()),
        Span::meta("b".to_string()),
        Span::muted("c".to_string()),
    ];

    // -- Check: Span constructors set the hint.
    assert_eq!(
        Span::normal("x".to_string()),
        Span {
            text: "x".to_string(),
            hint: StyleHint::Normal
        }
    );
    let arbitrary = Span::new("y".to_string(), StyleHint::DiffAdd);
    assert_eq!(arbitrary.text, "y");
    assert_eq!(arbitrary.hint, StyleHint::DiffAdd);

    // -- Check: ContentLine constructors build the expected spans.
    let single = ContentLine::single("hello".to_string(), StyleHint::Emphasis);
    assert_eq!(single.spans.len(), 1);
    assert_eq!(single.spans[0].text, "hello");
    assert_eq!(single.spans[0].hint, StyleHint::Emphasis);
    assert!(ContentLine::empty().spans.is_empty());
    let from_spans = ContentLine::from_spans(spans.clone());
    assert_eq!(from_spans.spans, spans);

    // -- Check: every constructor defaults diff_tint to None and hang_indent to 0.
    let empty = ContentLine::empty();
    let default = ContentLine::default();
    assert!(default.spans.is_empty());
    for line in [&single, &from_spans, &empty, &default] {
        assert_eq!(
            line.diff_tint, None,
            "constructor must default diff_tint to None"
        );
        assert_eq!(
            line.hang_indent, 0,
            "constructor must default hang_indent to 0"
        );
    }

    // -- Check: single_with_tint stores the tint.
    let tinted =
        ContentLine::single_with_tint("+added".to_string(), StyleHint::MdCodePlain, DiffTint::Add);
    assert_eq!(tinted.spans.len(), 1);
    assert_eq!(tinted.spans[0].text, "+added");
    assert_eq!(tinted.spans[0].hint, StyleHint::MdCodePlain);
    assert_eq!(tinted.diff_tint, Some(DiffTint::Add));

    // -- Check: DiffTint variants are distinct.
    assert_ne!(DiffTint::Add, DiffTint::Remove);
    assert_ne!(DiffTint::Add, DiffTint::Context);
    assert_ne!(DiffTint::Remove, DiffTint::Context);
}
