use super::*;

use crate::state::CompactionStatus;

/// In-place tool mutations re-measure only the mutated entry: the index stays
/// valid at the current width and its geometry equals a wholesale rebuild.
#[test]
fn test_transcript_store_set_tool_result_at_keeps_the_height_index_valid() -> Result<()> {
    // -- Setup & Fixtures: an indexed store whose tool entry has a preview.
    let width = 60;
    let mut state = AppState::default();
    push_user(&mut state, "run the edit");
    state.transcript.push_entry(tool_entry());
    state.transcript.rebuild_height_index(width);
    assert!(state.transcript.height_index_valid_for(width));

    // -- Exec: complete the tool with a tall diff display.
    let index = state.transcript.len() - 1;
    assert!(
        state.transcript.set_tool_result_at(index, diff_display()),
        "the entry must be a tool entry"
    );

    // -- Check: the index is still valid and matches a wholesale rebuild.
    assert!(
        state.transcript.height_index_valid_for(width),
        "an in-place result must not invalidate the height index"
    );
    assert_matches_wholesale(&state, width, "after set_tool_result_at")?;
    Ok(())
}

/// The preview path behaves like the result path: the mutated entry is
/// re-measured in place and the index stays valid.
#[test]
fn test_transcript_store_set_tool_preview_at_keeps_the_height_index_valid() -> Result<()> {
    // -- Setup & Fixtures
    let width = 48;
    let mut state = AppState::default();
    push_user(&mut state, "write the file");
    state.transcript.push_entry(tool_entry());
    state.transcript.rebuild_height_index(width);
    assert!(state.transcript.height_index_valid_for(width));

    // -- Exec: attach a multi-row code preview.
    let index = state.transcript.len() - 1;
    assert!(
        state.transcript.set_tool_preview_at(index, code_display()),
        "the entry must be a tool entry"
    );

    // -- Check
    assert!(
        state.transcript.height_index_valid_for(width),
        "an in-place preview must not invalidate the height index"
    );
    assert_matches_wholesale(&state, width, "after set_tool_preview_at")?;
    Ok(())
}

/// A stale index stays stale: a mutation before any rebuild reports no width
/// validity, and the next rebuild reproduces the wholesale geometry.
#[test]
fn test_transcript_store_in_place_mutation_does_not_fake_validity() -> Result<()> {
    // -- Setup & Fixtures: a store whose index was never built.
    let width = 60;
    let mut state = AppState::default();
    push_user(&mut state, "run the edit");
    state.transcript.push_entry(tool_entry());
    assert!(!state.transcript.height_index_valid_for(width));

    // -- Exec
    let index = state.transcript.len() - 1;
    assert!(state.transcript.set_tool_result_at(index, diff_display()));

    // -- Check: the mutation does not invent validity — and the next rebuild
    // yields the wholesale geometry.
    assert!(
        !state.transcript.height_index_valid_for(width),
        "a mutation must not report an index it never built"
    );
    state.transcript.rebuild_height_index(width);
    assert_matches_wholesale(&state, width, "after the rebuild")?;
    Ok(())
}

/// `Compaction::finish_block` carries the scanned row index into the
/// re-measure, so the task line's status change keeps the index valid.
#[test]
fn test_transcript_store_compaction_finish_block_keeps_the_height_index_valid() -> Result<()> {
    // -- Setup & Fixtures: an in-progress compaction block, indexed.
    let width = 72;
    let mut state = AppState::default();
    push_user(&mut state, "compact please");
    state.compaction.start_block(&mut state.transcript, "auto");
    state.transcript.rebuild_height_index(width);
    assert!(state.transcript.height_index_valid_for(width));

    // -- Exec
    state
        .compaction
        .finish_block(&mut state.transcript, "auto", CompactionStatus::Done);

    // -- Check: the task line moved to Done and the index stayed valid.
    assert!(
        state.transcript.height_index_valid_for(width),
        "an in-place task-status mutation must not invalidate the height index"
    );
    assert_matches_wholesale(&state, width, "after finish_block")?;
    Ok(())
}

// -- Test Support

/// Assert the store's index is valid at `width`, then compare its total, every
/// start row, and its whole frame plan against a wholesale rebuild of the same
/// entries.
fn assert_matches_wholesale(state: &AppState, width: usize, context: &str) -> Result<()> {
    assert!(
        state.transcript.height_index_valid_for(width),
        "index must stay valid {context}"
    );
    let mut wholesale = state.transcript.clone();
    wholesale.rebuild_height_index(width);
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale.total_visual_rows(),
        "total rows must equal a wholesale rebuild {context}"
    );
    for index in 0..state.transcript.len() {
        assert_eq!(
            state.transcript.start_row_of(index),
            wholesale.start_row_of(index),
            "start row of entry {index} must equal a wholesale rebuild {context}"
        );
    }
    let len = state.transcript.len();
    assert_eq!(
        state.transcript.frame_plan(0, len, width),
        wholesale.frame_plan(0, len, width),
        "frame plan must equal a wholesale rebuild {context}"
    );
    Ok(())
}

/// A tool entry with no display, ready for an in-place mutation.
fn tool_entry() -> TranscriptEntry {
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::Edit,
        call: ToolCall::new(
            ToolName("edit".to_string()),
            "",
            r#"{"path":"src/main.rs"}"#,
        ),
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    })
}

/// A multi-row diff display, tall enough that a stale measurement shows.
fn diff_display() -> Display {
    Display {
        title: Some("edit src/main.rs".to_string()),
        sections: vec![DisplaySection {
            label: Some("src/main.rs".to_string()),
            kind: ContentKind::Diff {
                language: "rust".to_string(),
                path: "src/main.rs".to_string(),
            },
            content: "@@ -1,3 +1,3 @@\n-old one\n-old two\n+new one\n+new two\n".to_string(),
            stats: None,
        }],
    }
}

/// A multi-row code display for the preview path.
fn code_display() -> Display {
    Display {
        title: Some("write src/main.rs".to_string()),
        sections: vec![DisplaySection {
            label: Some("src/main.rs".to_string()),
            kind: ContentKind::Code {
                language: "rust".to_string(),
            },
            content: "fn main() {\n    println!(\"one\");\n    println!(\"two\");\n    println!(\"three\");\n}\n"
                .to_string(),
            stats: None,
        }],
    }
}
