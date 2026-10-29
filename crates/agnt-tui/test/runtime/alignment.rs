//! End-to-end transcript gutters through the real render loop (task 45b99a84).
//!
//! Drives `RenderLoopDriver` with a hydrated user + assistant transcript and a
//! tool start published on the bus, then asserts the rendered buffer columns:
//! the dotted rail at list-local 0, the cog marker at list-local 2, entry text
//! at the cog column, and the list area's last two columns clear.

use agnt_core::protocol::contracts::UiMessageSnapshot;
use agnt_core::protocol::event::UiEvent;
use agnt_core::protocol::tool_args::CallLine;

use crate::runtime::test_driver::{DriveEvent, RenderLoopDriver};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Terminal width: 100 columns leaves a 96-column transcript list area (1
/// column of side margin each side, then 2 more columns of list inset).
const COLUMNS: u16 = 100;
const ROWS: u16 = 30;

const RAIL: char = '\u{254e}';
const COG: char = '\u{2699}';

/// WHEN the render loop renders a user, assistant, and tool transcript, THE
/// user rail SHALL sit at list-local 0 with user text at list-local 2, THE cog
/// marker SHALL sit at list-local 2 with tool text after it, THE assistant text
/// SHALL start at list-local 2, and no transcript row SHALL render text in the
/// list area's last two columns.
#[tokio::test]
async fn test_alignment_transcript_gutters_pin_rail_cog_and_right_margin() -> Result<()> {
    // -- Setup & Fixtures: hydrated prose long enough to wrap, then a tool
    // started through the bus.
    let mut driver = RenderLoopDriver::new(COLUMNS, ROWS);
    driver.coordinator_mut().hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", prose("user")),
            UiMessageSnapshot::new("assistant", prose("assist")),
        ],
        None,
    );

    // -- Exec
    driver
        .advance_with_frame(&[DriveEvent::UiEvent(Box::new(UiEvent::ToolStarted {
            name: "tool".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            call_line: CallLine::from_json_summary("{}"),
        }))])
        .await?;

    // -- Check: list-local 0 is the list area's first column.
    let buffer = driver.buffer_text();
    let rows: Vec<&str> = buffer.lines().collect();
    let area = driver.state().scroll.transcript_area;
    let list_x = area.x as usize;
    let list_width = (area.width as usize).saturating_sub(2);
    assert!(list_width > 3, "fixture needs a list area wider than 3");

    // -- Check: the user rail sits at list-local 0 on row 0 and every
    // continuation row, with text at list-local 2.
    let user_rows: Vec<(usize, &&str)> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.contains("user0"))
        .collect();
    assert!(
        !user_rows.is_empty(),
        "the user block must render, got:\n{buffer}"
    );
    for (y, row) in &user_rows {
        assert_eq!(
            row.chars().nth(list_x),
            Some(RAIL),
            "row {y} must carry the dotted rail at list-local 0; got {row:?}"
        );
        assert_eq!(
            row.chars().nth(list_x + 2),
            Some('u'),
            "row {y} must start its text at list-local 2; got {row:?}"
        );
    }

    // -- Check: the assistant text starts at list-local 2 with no cursor slot.
    let assistant_row = rows
        .iter()
        .find(|row| row.contains("assist0"))
        .ok_or("the assistant block must render")?;
    assert_eq!(
        assistant_row.chars().nth(list_x + 2),
        Some('a'),
        "assistant text must start at list-local 2; got {assistant_row:?}"
    );
    assert!(
        !assistant_row.contains(RAIL),
        "the assistant lane must not carry the user rail; got {assistant_row:?}"
    );

    // -- Check: the cog sits at list-local 2. A started tool carries the 2-col
    // status indicator after the marker slot, so its call line starts at
    // list-local 6.
    let tool_row = rows
        .iter()
        .find(|row| row.contains(COG))
        .ok_or("the tool block must render")?;
    let tool_chars: Vec<char> = tool_row.chars().collect();
    assert_eq!(
        tool_chars.get(list_x + 2),
        Some(&COG),
        "the cog must sit at list-local 2; got {tool_row:?}"
    );
    let call_line: String = tool_chars.iter().skip(list_x + 6).collect();
    assert!(
        call_line.starts_with("tool"),
        "marker 2 + indicator 2 puts the tool call line at list-local 6; got {tool_row:?}"
    );

    // -- Check: no transcript row writes into the list area's last two columns.
    let max_column = list_x + list_width - 3;
    assert!(list_width > 0, "the transcript list area must have a width");
    for y in area.y as usize..(area.y as usize + area.height as usize) {
        let Some(row) = rows.get(y) else {
            continue;
        };
        let Some(last) = row.trim_end().chars().count().checked_sub(1) else {
            continue;
        };
        assert!(
            last <= max_column,
            "row {y} must stop at column {max_column}; got text at {last}: {row:?}"
        );
    }
    Ok(())
}

/// WHEN the transcript renders a user turn bracketed by assistant turns, THE
/// user background box's first and last rows — the bleed separator rows — SHALL
/// carry the `╎` rail at list-local 0 on `row_user_bg`, not only the
/// text-bearing rows (task 4d99956c).
#[tokio::test]
async fn test_alignment_user_box_first_and_last_row_carry_the_rail() -> Result<()> {
    // -- Setup & Fixtures: assistant above, a wrapping user turn, assistant
    // below, so the user box has one bleed separator row on each side.
    let mut driver = RenderLoopDriver::new(COLUMNS, ROWS);
    driver.coordinator_mut().hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("assistant", prose("above")),
            UiMessageSnapshot::new("user", prose("user")),
            UiMessageSnapshot::new("assistant", prose("below")),
        ],
        None,
    );

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: list-local 0 is the list area's first column.
    let theme = driver.coordinator().theme.clone();
    let buffer = driver.buffer_text();
    let rows: Vec<&str> = buffer.lines().collect();
    let area = driver.state().scroll.transcript_area;
    let list_x = area.x as usize;
    let first_text = rows
        .iter()
        .position(|row| row.contains("user00000"))
        .ok_or("the user block's first row must render")?;
    let last_text = rows
        .iter()
        .position(|row| row.contains("user00011"))
        .ok_or("the user block's last row must render")?;
    assert!(
        last_text > first_text,
        "the fixture requires the user turn to wrap, got:\n{buffer}"
    );
    for y in [first_text - 1, last_text + 1] {
        let row = rows.get(y).ok_or("the bleed row must render")?;
        assert_eq!(
            row.chars().nth(list_x),
            Some(RAIL),
            "row {y} is the box edge and must carry the rail at list-local 0; got {row:?}"
        );
        assert_eq!(
            row.chars().nth(list_x + 2),
            Some(' '),
            "row {y} is a bleed row and must carry no text; got {row:?}"
        );
        let style = driver.cell_style_at(area.x, y as u16);
        assert_eq!(
            style.bg,
            Some(theme.row_user_bg),
            "row {y} must paint row_user_bg"
        );
        assert_eq!(
            style.fg, theme.lane_prefix_user.fg,
            "row {y} must paint the rail in the lane_prefix_user colour"
        );
    }
    Ok(())
}

// -- Test Support

/// Prose long enough to wrap at the 96-column list area's budget: twelve
/// nine-character words sharing a `prefix`.
fn prose(prefix: &str) -> String {
    (0..12)
        .map(|index| format!("{prefix}{index:05}"))
        .collect::<Vec<String>>()
        .join(" ")
}
