use super::viewport::TranscriptViewport;

#[derive(Clone, Copy)]
enum Op {
    LineUp,
    LineDown,
    PageUp(usize),
    PageDown(usize),
    Top,
    Bottom,
}

fn run_ops(model: &mut TranscriptViewport, ops: &[Op]) {
    for op in ops {
        match *op {
            Op::LineUp => model.line_up(),
            Op::LineDown => model.line_down(),
            Op::PageUp(lines) => model.page_up(lines),
            Op::PageDown(lines) => model.page_down(lines),
            Op::Top => model.jump_top(),
            Op::Bottom => model.jump_bottom(),
        }
    }
}

#[test]
fn gg_sets_cursor_to_first_line() {
    let mut model = TranscriptViewport::new(10, 3);

    run_ops(&mut model, &[Op::Bottom, Op::Top]);

    assert_eq!(model.current_cursor_index(), Some(0));
    assert!(!model.follow_tail());
    assert_eq!(model.offset_from_bottom(), model.max_offset_from_bottom());
}

#[test]
fn g_sets_cursor_to_last_line() {
    let mut model = TranscriptViewport::new(10, 3);

    run_ops(&mut model, &[Op::Top, Op::Bottom]);

    assert_eq!(model.current_cursor_index(), Some(9));
    assert!(model.follow_tail());
    assert_eq!(model.offset_from_bottom(), 0);
}

#[test]
fn motions_after_jumps_preserve_continuity_table_driven() {
    struct Case {
        name: &'static str,
        len: usize,
        viewport: usize,
        ops: Vec<Op>,
        expected_cursor: Option<usize>,
        expected_follow_tail: bool,
        expected_offset: usize,
    }

    let cases = vec![
        Case {
            name: "top_then_line_down",
            len: 10,
            viewport: 3,
            ops: vec![Op::Top, Op::LineDown],
            expected_cursor: Some(1),
            expected_follow_tail: false,
            expected_offset: 7,
        },
        Case {
            name: "top_then_page_down",
            len: 10,
            viewport: 3,
            ops: vec![Op::Top, Op::PageDown(4)],
            expected_cursor: Some(4),
            expected_follow_tail: false,
            expected_offset: 5,
        },
        Case {
            name: "bottom_then_line_up",
            len: 10,
            viewport: 3,
            ops: vec![Op::Bottom, Op::LineUp],
            expected_cursor: Some(8),
            expected_follow_tail: false,
            expected_offset: 0,
        },
        Case {
            name: "bottom_then_page_up",
            len: 10,
            viewport: 3,
            ops: vec![Op::Bottom, Op::PageUp(4)],
            expected_cursor: Some(5),
            expected_follow_tail: false,
            expected_offset: 2,
        },
        Case {
            name: "short_transcript_stays_clamped",
            len: 2,
            viewport: 10,
            ops: vec![Op::Top, Op::PageDown(8), Op::LineUp],
            expected_cursor: Some(0),
            expected_follow_tail: false,
            expected_offset: 0,
        },
    ];

    for case in cases {
        let mut model = TranscriptViewport::new(case.len, case.viewport);
        run_ops(&mut model, &case.ops);

        assert_eq!(
            model.current_cursor_index(),
            case.expected_cursor,
            "case: {}",
            case.name
        );
        assert_eq!(
            model.follow_tail(),
            case.expected_follow_tail,
            "case: {}",
            case.name
        );
        assert_eq!(
            model.offset_from_bottom(),
            case.expected_offset,
            "case: {}",
            case.name
        );
    }
}

#[test]
fn bottom_cursor_moves_within_viewport_before_scrolling_window_up() {
    let mut model = TranscriptViewport::new(10, 3);
    run_ops(&mut model, &[Op::Bottom]);

    model.line_up();
    assert_eq!(model.current_cursor_index(), Some(8));
    assert_eq!(model.offset_from_bottom(), 0);

    model.line_up();
    assert_eq!(model.current_cursor_index(), Some(7));
    assert_eq!(model.offset_from_bottom(), 0);

    model.line_up();
    assert_eq!(model.current_cursor_index(), Some(6));
    assert_eq!(model.offset_from_bottom(), 1);
}

#[test]
fn top_cursor_moves_within_viewport_before_scrolling_window_down() {
    let mut model = TranscriptViewport::new(10, 3);
    run_ops(&mut model, &[Op::Top]);

    model.line_down();
    assert_eq!(model.current_cursor_index(), Some(1));
    assert_eq!(model.offset_from_bottom(), 7);

    model.line_down();
    assert_eq!(model.current_cursor_index(), Some(2));
    assert_eq!(model.offset_from_bottom(), 7);

    model.line_down();
    assert_eq!(model.current_cursor_index(), Some(3));
    assert_eq!(model.offset_from_bottom(), 6);
}

#[test]
fn empty_transcript_is_safe_and_has_no_cursor() {
    let mut model = TranscriptViewport::new(0, 5);

    run_ops(
        &mut model,
        &[Op::Top, Op::Bottom, Op::LineUp, Op::LineDown, Op::PageUp(3), Op::PageDown(3)],
    );

    assert_eq!(model.current_cursor_index(), None);
    assert!(model.follow_tail());
    assert_eq!(model.offset_from_bottom(), 0);
    assert_eq!(model.max_offset_from_bottom(), 0);
}

#[test]
fn viewport_model_never_uses_usize_max_as_sentinel() {
    let mut model = TranscriptViewport::new(10, 3);
    run_ops(
        &mut model,
        &[
            Op::Top,
            Op::PageDown(100),
            Op::Bottom,
            Op::PageUp(100),
            Op::LineDown,
            Op::LineUp,
        ],
    );

    assert_ne!(model.offset_from_bottom(), usize::MAX);
    assert!(model.offset_from_bottom() <= model.max_offset_from_bottom());
    if let Some(cursor) = model.current_cursor_index() {
        assert_ne!(cursor, usize::MAX);
        assert!(cursor < model.transcript_len());
    }
}

#[test]
fn line_up_from_follow_tail_detaches_and_moves_cursor() {
    let mut model = TranscriptViewport::new(10, 3);
    assert!(model.follow_tail());
    assert_eq!(model.current_cursor_index(), Some(9));

    model.line_up();

    assert!(!model.follow_tail());
    assert_eq!(model.current_cursor_index(), Some(8));
    assert_eq!(model.offset_from_bottom(), 0);
}

#[test]
fn page_up_from_follow_tail_detaches_and_moves_cursor() {
    let mut model = TranscriptViewport::new(10, 3);
    assert!(model.follow_tail());
    assert_eq!(model.current_cursor_index(), Some(9));

    model.page_up(3);

    assert!(!model.follow_tail());
    assert_eq!(model.current_cursor_index(), Some(6));
}
