use super::support::*;
use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::MessageRole;

// ---------------------------------------------------------------------------
// User prose
// ---------------------------------------------------------------------------

/// WHEN the message widget renders a user turn, THE row SHALL carry the user
/// surface across the whole pane and the rail SHALL sit in the gutter.
#[test]
fn test_widgets_message_user_paints_surface_and_rail() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "Hello there");
    let theme = test_theme();

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    assert_eq!(row_text(&backend, 0).trim_end(), "╎ Hello there");
    for x in [0u16, 1, 10, WIDTH - 1] {
        assert_eq!(
            cell_bg(&backend, x, 0),
            Some(terminal(theme.user_surface)),
            "the user surface covers column {x}"
        );
    }
    assert_eq!(cell_fg(&backend, 2, 0), Some(terminal(theme.text)));
    Ok(())
}

/// WHEN a user turn wraps, THE rail SHALL stay in the gutter on every row and
/// the user surface SHALL cover every row.
#[test]
fn test_widgets_message_user_rail_survives_wrapped_rows() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    let entry = message_entry(MessageRole::User, markdown);
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert!(rows > 1, "the fixture must wrap, got {rows} rows");
    for y in 0..u16::try_from(rows)? {
        assert!(
            row_text(&backend, y).starts_with("╎ "),
            "row {y} keeps the rail: {:?}",
            row_text(&backend, y)
        );
        assert_eq!(
            cell_bg(&backend, WIDTH - 1, y),
            Some(terminal(theme.user_surface)),
            "row {y} keeps the user surface to the pane edge"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Assistant prose
// ---------------------------------------------------------------------------

/// WHEN the message widget renders an assistant turn, THE row SHALL carry a
/// blank gutter and plain text with no surface background.
#[test]
fn test_widgets_message_assistant_is_untinted_prose() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::Assistant, "Hello there");
    let theme = test_theme();

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    assert_eq!(row_text(&backend, 0).trim_end(), "  Hello there");
    assert!(
        is_untinted(cell_bg(&backend, 0, 0)),
        "assistant prose is untinted"
    );
    assert!(is_untinted(cell_bg(&backend, WIDTH - 1, 0)));
    assert_eq!(cell_fg(&backend, 2, 0), Some(terminal(theme.text)));
    Ok(())
}

// ---------------------------------------------------------------------------
// The footer row
// ---------------------------------------------------------------------------

/// One stamp every footer fixture shares (2025-12-04T22:13:20Z).
const STAMP: i64 = 1_765_000_000_000;

/// The blank margin row between the content and the footer, and the footer row
/// itself, in every one-content-row stamped fixture.
const GAP_ROW: u16 = 1;
const FOOTER_ROW: u16 = 2;

/// WHEN a stamped entry paints, THE widget SHALL leave exactly one blank row
/// between the body and the footer — the margin the user asked for — painted in
/// the user surface style with no rail and no content.
#[test]
fn test_widgets_message_footer_gap_row_separates_body_from_footer() -> Result<()> {
    // -- Setup & Fixtures
    let entry = stamped_message_entry(
        MessageRole::User,
        "Hello there",
        Some(STAMP),
        Some("openai/gpt-5"),
    );
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: content, gap, footer.
    assert_eq!(rows, 3, "content + the gap row + the footer row");
    assert_eq!(
        row_text(&backend, GAP_ROW),
        format!("╎ {}", " ".repeat(usize::from(WIDTH) - 2)),
        "the gap row keeps the rail on the lane and stays blank beside it"
    );
    for x in [0u16, 1, WIDTH - 3, WIDTH - 1] {
        assert_eq!(
            cell_bg(&backend, x, GAP_ROW),
            Some(terminal(theme.user_surface)),
            "the user surface carries the gap row at column {x}"
        );
    }
    assert!(
        backend.buffer()[(0, FOOTER_ROW)].symbol().starts_with('╎'),
        "the footer keeps its rail below the gap"
    );
    Ok(())
}

/// WHEN a user entry carries its stamp and model, THE widget SHALL measure one
/// row more than its content and paint the right-aligned muted footer there.
#[test]
fn test_widgets_message_user_footer_is_one_right_aligned_row() -> Result<()> {
    // -- Setup & Fixtures
    let entry = stamped_message_entry(
        MessageRole::User,
        "Hello there",
        Some(STAMP),
        Some("openai/gpt-5"),
    );
    let theme = test_theme();
    let footer = format!("openai/gpt-5 · {}", stamp_text(STAMP));

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: the content row, then exactly one footer row.
    assert_eq!(rows, 3, "content + the gap row + the footer row");
    assert_eq!(
        row_text(&backend, FOOTER_ROW).trim_end(),
        format!("╎ {footer:>36}"),
        "the footer is right-aligned in the content budget"
    );
    assert_eq!(
        cell_fg(&backend, WIDTH - 3, FOOTER_ROW),
        Some(terminal(theme.muted)),
        "the footer text is muted"
    );
    assert_eq!(
        cell_bg(&backend, WIDTH - 1, FOOTER_ROW),
        Some(terminal(theme.user_surface)),
        "the user surface continues under the footer"
    );
    assert!(
        row_text(&backend, FOOTER_ROW).starts_with("╎ "),
        "the rail continues on the footer row"
    );
    Ok(())
}

/// A timestamp alone is a valid footer: the composition skips the absent
/// segment rather than inventing one.
#[test]
fn test_widgets_message_user_footer_timestamp_only() -> Result<()> {
    // -- Setup & Fixtures
    let entry = stamped_message_entry(MessageRole::User, "Hello there", Some(STAMP), None);
    let stamp = stamp_text(STAMP);

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 3);
    assert_eq!(
        row_text(&backend, FOOTER_ROW).trim_end(),
        format!("╎ {stamp:>36}")
    );
    assert!(
        !row_text(&backend, FOOTER_ROW).contains('·'),
        "no separator without a second segment"
    );
    Ok(())
}

/// A model alone is a valid footer too.
#[test]
fn test_widgets_message_user_footer_model_only() -> Result<()> {
    // -- Setup & Fixtures
    let entry = stamped_message_entry(MessageRole::User, "Hello there", None, Some("openai/gpt-5"));

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 3);
    assert_eq!(
        row_text(&backend, FOOTER_ROW).trim_end(),
        format!("╎ {:>36}", "openai/gpt-5")
    );
    Ok(())
}

/// WHEN the entry carries no fact, THE widget SHALL render exactly as it does
/// without a footer: one row, nothing beneath the prose.
#[test]
fn test_widgets_message_user_without_facts_has_no_footer() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "Hello there");

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1, "no facts, no extra row");
    assert!(
        row_text(&backend, 1).trim().is_empty(),
        "the row below the prose stays blank"
    );
    Ok(())
}

/// WHEN an assistant entry carries facts, THE widget SHALL still render no
/// footer: the footer belongs to the user turn.
#[test]
fn test_widgets_message_assistant_stamped_has_no_footer() -> Result<()> {
    // -- Setup & Fixtures
    let entry = stamped_message_entry(
        MessageRole::Assistant,
        "Hello there",
        Some(STAMP),
        Some("openai/gpt-5"),
    );

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1, "an assistant entry keeps its one content row");
    assert!(
        row_text(&backend, 1).trim().is_empty(),
        "no footer under assistant prose"
    );
    Ok(())
}

/// WHEN the composed footer is wider than the budget, THE row SHALL clip at the
/// budget: no wrap, no second row.
#[test]
fn test_widgets_message_footer_clips_at_the_budget() -> Result<()> {
    // -- Setup & Fixtures: a model identity far wider than the 36-column
    // budget (40 columns minus the 2-column rail and the 2-column margin).
    let model = "a".repeat(60);
    let entry = stamped_message_entry(MessageRole::User, "Hello there", None, Some(&model));

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 3, "a clipped footer still costs the gap plus one row");
    assert_eq!(
        row_text(&backend, FOOTER_ROW).trim_end(),
        format!("╎ {}", "a".repeat(36)),
        "the row stops at the budget, before the right margin"
    );
    for x in [WIDTH - 2, WIDTH - 1] {
        assert_eq!(
            backend.buffer()[(x, FOOTER_ROW)].symbol(),
            " ",
            "column {x} stays inside the right margin"
        );
    }
    Ok(())
}

/// The suite's stamped entry, with the agent segment set: the persona identity
/// the entry carries, or none.
fn agent_entry(
    timestamp_millis: Option<i64>,
    model: Option<&str>,
    agent: Option<&str>,
) -> TranscriptEntry {
    let mut entry =
        stamped_message_entry(MessageRole::User, "Hello there", timestamp_millis, model);
    let TranscriptEntry::Message(message) = &mut entry else {
        return entry;
    };
    message.agent = agent.map(str::to_string);
    entry
}

/// WHEN the entry carries a stamp, an agent, and a model, THE footer SHALL
/// compose all three in the ruled order: agent, model, stamp.
#[test]
fn test_widgets_message_footer_carries_the_agent_between_stamp_and_model() -> Result<()> {
    // -- Setup & Fixtures: a model short enough for all three segments to fit
    // the 36-column budget (the clip itself is pinned by its own test).
    let entry = agent_entry(Some(STAMP), Some("gpt-5"), Some("coder"));
    let footer = format!("coder · gpt-5 · {}", stamp_text(STAMP));

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 3, "content + the gap row + the footer row");
    assert_eq!(
        row_text(&backend, FOOTER_ROW).trim_end(),
        format!("╎ {footer:>36}"),
        "the agent sits between the stamp and the model"
    );
    Ok(())
}

/// WHEN the entry carries a stamp and an agent only, THE footer SHALL render
/// the two segments in the ruled order and skip the absent model.
#[test]
fn test_widgets_message_footer_renders_stamp_and_agent() -> Result<()> {
    // -- Setup & Fixtures
    let entry = agent_entry(Some(STAMP), None, Some("coder"));
    let footer = format!("coder · {}", stamp_text(STAMP));

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 3);
    assert_eq!(
        row_text(&backend, FOOTER_ROW).trim_end(),
        format!("╎ {footer:>36}")
    );
    Ok(())
}

/// WHEN the agent is absent, THE footer SHALL render the remaining segments in
/// the ruled order: model, then stamp, no separator where nothing was skipped.
#[test]
fn test_widgets_message_footer_without_an_agent_keeps_two_segments() -> Result<()> {
    // -- Setup & Fixtures
    let entry = agent_entry(Some(STAMP), Some("openai/gpt-5"), None);
    let footer = format!("openai/gpt-5 · {}", stamp_text(STAMP));

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 3);
    assert_eq!(
        row_text(&backend, FOOTER_ROW).trim_end(),
        format!("╎ {footer:>36}"),
        "a missing agent leaves no gap in the composition"
    );
    Ok(())
}
