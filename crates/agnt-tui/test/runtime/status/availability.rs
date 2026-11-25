//! Command availability on the surface: the row a gated command renders, and the
//! fact it derives from.

use crate::rendering::theme::TuiTheme;
use crate::state::AppState;
use agnt_runtime::bus::{CommandFacts, CompactionAvailability, FactRecord};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// A reason the runtime could ship: the availability carries content, and the
/// surface prints it verbatim.
const REASON: &str = "nothing left to cut.";

/// The pair a conversation that ends in its compaction answers.
fn compacted() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Unavailable(REASON.to_string()),
        uncompact: CompactionAvailability::Available,
    }
}

/// The pair a never-compacted conversation answers.
fn never_compacted() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Available,
        uncompact: CompactionAvailability::Unavailable(REASON.to_string()),
    }
}

/// The pair a conversation a sweep can cut and a summary waits to undo answers:
/// both gated commands can run.
fn both_offered() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Available,
        uncompact: CompactionAvailability::Available,
    }
}

// endregion: --- Fixtures

/// WHEN the session ends in its most recent compaction, THE suggestion row SHALL
/// keep its label and summary and SHALL carry the picker's dimmed unavailable
/// style: the style alone says the command cannot run now. The undo carries its
/// own answer in the same fact, so it stays offered.
#[test]
fn test_runtime_status_slash_availability_dims_the_unavailable_row() -> Result<()> {
    // -- Setup & Fixtures: the inline slash list open on the full command set.
    let mut state = AppState::default();
    state.check_inline_slash("/");
    let theme = TuiTheme::default();

    // -- Exec: the runtime answers that nothing is left to cut.
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(compacted()))
    );

    // -- Check: the row keeps its own words, carries no reason text, and every
    // span carries the dimmed style.
    let rows = inline_slash_row_styles(&state);
    let (text, spans) = rows
        .iter()
        .find(|(text, _)| text.contains("/compact"))
        .ok_or("the /compact row must stay listed")?;
    assert!(
        text.contains("Force compaction now"),
        "the unavailable row keeps its summary: {text:?}"
    );
    assert!(
        !text.contains(REASON),
        "the unavailable row carries no reason text: {text:?}"
    );
    assert!(!spans.is_empty(), "the row paints at least one span");
    for (span, style) in spans {
        assert_eq!(
            *style, theme.picker_row_unavailable,
            "every span of the unavailable row is dimmed: {span:?} in {text:?}"
        );
    }

    // -- Check: the pair carries the other command's answer, and its label keeps
    // the label style.
    let (other, other_spans) = rows
        .iter()
        .find(|(text, _)| text.contains("/uncompact"))
        .ok_or("the /uncompact row must stay listed")?;
    assert!(
        other.contains("Undo the newest compaction"),
        "the other command keeps its summary: {other:?}"
    );
    assert!(
        other_spans
            .iter()
            .any(|(span, style)| span.contains("/uncompact") && *style == theme.subtle_meta),
        "the row's own answer keeps its label undimmed: {other:?}"
    );
    Ok(())
}

/// WHEN a compaction waits to be undone, THE `/uncompact` row SHALL dim and THE
/// `/compact` row SHALL stay offered: each command reads its own arm of the same
/// pair.
#[test]
fn test_runtime_status_slash_availability_reads_the_uncompact_arm() -> Result<()> {
    // -- Setup & Fixtures: the inline slash list open on the full command set.
    let mut state = AppState::default();
    state.check_inline_slash("/");
    let theme = TuiTheme::default();

    // -- Exec: the runtime answers from the pair the undo's gate derives from.
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(never_compacted()))
    );

    // -- Check: the undo's row is dimmed on its own answer, not on the sweep's.
    let rows = inline_slash_row_styles(&state);
    let (undo, undo_spans) = rows
        .iter()
        .find(|(text, _)| text.contains("/uncompact"))
        .ok_or("the /uncompact row must stay listed")?;
    assert!(!undo_spans.is_empty(), "the row paints at least one span");
    for (span, style) in undo_spans {
        assert_eq!(
            *style, theme.picker_row_unavailable,
            "every span of the unavailable undo row is dimmed: {span:?} in {undo:?}"
        );
    }

    // -- Check: the sweep's arm is available, so its row keeps its label style.
    let (compact, compact_spans) = rows
        .iter()
        .find(|(text, _)| text.contains("/compact"))
        .ok_or("the /compact row must stay listed")?;
    assert!(
        compact_spans
            .iter()
            .any(|(span, style)| span.contains("/compact") && *style == theme.subtle_meta),
        "the sweep's row stays offered: {compact:?}"
    );
    Ok(())
}

/// WHEN every offered command can run, THE rows SHALL render without the
/// unavailable style: the normal case is untouched.
#[test]
fn test_runtime_status_slash_availability_leaves_available_rows_undimmed() -> Result<()> {
    // -- Setup & Fixtures: the runtime answers that both commands have work.
    let mut state = AppState::default();
    state.check_inline_slash("/");
    let theme = TuiTheme::default();
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(both_offered()))
    );

    // -- Exec
    let rows = inline_slash_row_styles(&state);

    // -- Check: the gated command's row is offered in full, and its label carries
    // the row's own label style rather than the dim one.
    let (text, spans) = rows
        .iter()
        .find(|(text, _)| text.contains("/compact"))
        .ok_or("the /compact row must be listed")?;
    assert!(
        text.contains("Force compaction now"),
        "an available command offers its own summary: {text:?}"
    );
    assert!(
        spans
            .iter()
            .any(|(span, style)| span.contains("/compact") && *style == theme.subtle_meta),
        "an available command's label renders in the label style: {text:?}"
    );
    assert!(
        !spans.iter().any(
            |(span, style)| span.contains("/compact") && *style == theme.picker_row_unavailable
        ),
        "an available command's label is not dimmed: {text:?}"
    );
    Ok(())
}

/// WHEN a surface holds no answer from the runtime yet, THE two gated rows SHALL
/// both render dimmed: a session nothing was read from offers neither command,
/// so the pre-fact window cannot show a row that would do nothing.
#[test]
fn test_runtime_status_slash_availability_dims_both_rows_before_the_fact() -> Result<()> {
    // -- Setup & Fixtures: the inline slash list open with no fact folded.
    let mut state = AppState::default();
    state.check_inline_slash("/");
    let theme = TuiTheme::default();

    // -- Exec
    let rows = inline_slash_row_styles(&state);

    // -- Exec & Check: each gated row paints every span in the dim style.
    for label in ["/compact", "/uncompact"] {
        let (text, spans) = rows
            .iter()
            .find(|(text, _)| text.contains(label))
            .ok_or_else(|| format!("the {label} row must stay listed"))?;
        assert!(
            !spans.is_empty(),
            "the row paints at least one span: {text:?}"
        );
        for (span, style) in spans {
            assert_eq!(
                *style, theme.picker_row_unavailable,
                "every span of the {label} row is dimmed: {span:?} in {text:?}"
            );
        }
    }
    Ok(())
}

/// WHEN the availability fact is folded twice with the same answer, THEN THE
/// surface SHALL change on the first and SHALL NOT on the second: a republished
/// snapshot is not a redraw.
#[test]
fn test_runtime_status_availability_folds_the_published_fact_once() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Check: a surface starts offering neither command.
    assert_eq!(
        state.status.command_availability,
        CommandFacts::default(),
        "a surface with no answer offers neither command"
    );

    // -- Exec & Check: the first answer changes the display, the repeat does not.
    let fact = FactRecord::CommandAvailability(never_compacted());
    assert!(state.status.reduce_fact_record(fact.clone()));
    assert_eq!(state.status.command_availability, never_compacted());
    assert!(
        !state.status.reduce_fact_record(fact),
        "the same answer twice changes nothing"
    );
    Ok(())
}

// region:    --- Test Support

/// The inline-slash rows with each span's text and style: the row text alone
/// cannot say how the row is drawn.
fn inline_slash_row_styles(
    state: &AppState,
) -> Vec<(String, Vec<(String, ratatui::style::Style)>)> {
    crate::runtime::status::help::inline_slash_lines_for_render(state, &TuiTheme::default())
        .into_iter()
        .map(|line| {
            let text = line.to_string();
            let spans = line
                .spans
                .into_iter()
                .map(|span| (span.content.to_string(), span.style))
                .collect();
            (text, spans)
        })
        .collect()
}

// endregion: --- Test Support
