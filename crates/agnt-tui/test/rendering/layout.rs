use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use ratatui_textarea::{CursorMove, TextArea, WrapMode};

use crate::rendering::layout::{INPUT_MAX_HEIGHT, input_content_row_count};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The input textarea the coordinator builds: word-or-glyph soft wrap, one
/// logical line per `\n`.
fn input_textarea(input: &str) -> TextArea<'static> {
    let mut textarea = TextArea::new(input.split('\n').map(str::to_string).collect());
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea
}

/// The same textarea with the caret parked at the end of the text.
fn input_textarea_at_end(input: &str) -> TextArea<'static> {
    let lines: Vec<String> = input.split('\n').map(str::to_string).collect();
    let last_row = lines.len().saturating_sub(1);
    let last_col = lines.get(last_row).map_or(0, |line| line.chars().count());
    let mut textarea = input_textarea(input);
    textarea.move_cursor(CursorMove::Jump(last_row as u16, last_col as u16));
    textarea
}

/// The visual row count the widget itself needs at `width`, read from its own
/// scroll behaviour: with the caret at the end of the text, a render at height
/// `h` needs no scrolling exactly when `h` covers every visual row. `None` means
/// the text needs more rows than the input box can show.
fn widget_rendered_rows(input: &str, width: u16) -> Option<u16> {
    for height in 1..=INPUT_MAX_HEIGHT {
        let textarea = input_textarea_at_end(input);
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        textarea.render(area, &mut buffer);
        if textarea.scroll_offset().0 == 0 {
            return Some(height);
        }
    }
    None
}

/// WHEN the input wraps at word boundaries, THE input_content_row_count
/// function SHALL count the visual rows the wrap produces.
#[test]
fn test_rendering_layout_input_content_row_count_counts_word_wrap() -> Result<()> {
    // -- Setup & Fixtures
    let fixtures = [
        ("", 8u16, 1u16),
        ("hello", 8, 1),
        ("hello world foo bar", 8, 3),
        ("hello world foo bar baz qux", 16, 2),
    ];

    // -- Exec & Check
    for (input, width, expected) in fixtures {
        assert_eq!(
            input_content_row_count(&input_textarea(input), width),
            expected,
            "input {input:?} must wrap to {expected} rows at width {width}"
        );
    }
    Ok(())
}

/// WHEN a word is wider than the content width, THE input_content_row_count
/// function SHALL count the grapheme rows the word falls back to, and a `\n`
/// SHALL start a new visual row.
#[test]
fn test_rendering_layout_input_content_row_count_counts_glyph_wrap_and_newlines() -> Result<()> {
    // -- Setup & Fixtures
    let long_word = "x".repeat(300);
    let fixtures = [
        ("helloworld", 4u16, 3u16),
        ("ab\n12345", 3, 3),
        ("a\n\n\n", 8, 4),
        ("mixed中文字符wrap", 6, 3),
        (long_word.as_str(), 4, 75),
    ];

    // -- Exec & Check
    for (input, width, expected) in fixtures {
        assert_eq!(
            input_content_row_count(&input_textarea(input), width),
            expected,
            "input {input:?} must wrap to {expected} rows at width {width}"
        );
    }
    Ok(())
}

/// WHEN the input_content_row_count function counts rows, THE count SHALL equal
/// the number of visual rows the textarea widget renders at the same width. The
/// count is read from the widget's own wrap state, so any drift between the two
/// fails here.
#[test]
fn test_rendering_layout_input_content_row_count_matches_widget_rendering() -> Result<()> {
    // -- Setup & Fixtures
    let long_word = "y".repeat(64);
    let inputs = [
        "",
        "hello",
        "hello world foo bar",
        "helloworld",
        "ab\n12345",
        "a\n\n\n",
        "one two three four five six",
        "mixed中文字符和ASCIIabc",
        long_word.as_str(),
    ];
    let widths = [3u16, 4, 6, 8, 16, 40];

    // -- Exec & Check
    for input in inputs {
        for width in widths {
            let counted = input_content_row_count(&input_textarea(input), width);
            match widget_rendered_rows(input, width) {
                Some(rendered) => assert_eq!(
                    counted, rendered,
                    "count must match the widget's rendered rows for {input:?} at width {width}"
                ),
                None => assert!(
                    counted > INPUT_MAX_HEIGHT,
                    "a text needing more than {INPUT_MAX_HEIGHT} rows must count above it for {input:?} at width {width}; counted {counted}"
                ),
            }
        }
    }
    Ok(())
}

/// WHEN the input is empty, THE input_content_row_count function SHALL report
/// one row, never zero, so the input box keeps its minimum height.
#[test]
fn test_rendering_layout_input_content_row_count_keeps_one_row_minimum() -> Result<()> {
    // -- Setup & Fixtures
    let widths = [1u16, 2, 80];

    // -- Exec & Check
    for width in widths {
        assert_eq!(
            input_content_row_count(&input_textarea(""), width),
            1,
            "an empty input must report one row at width {width}"
        );
    }
    Ok(())
}
