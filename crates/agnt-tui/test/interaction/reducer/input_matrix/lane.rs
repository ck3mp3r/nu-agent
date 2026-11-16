//! Rows 14-15: the key lane survives the work that queues beside it.
//!
//! Both rows pin the consequence the fold can see. The ordering claim they serve
//! — the key folds before any record fold in one pass — is the render loop's
//! (`interactive/render_loop.rs`) and its driver pin is 52feb274's.

use super::*;

// row 14 — a text-lane burst does not stop the key from folding.
#[test]
fn test_input_matrix_key_folds_after_text_lane_burst() -> Result<()> {
    // -- Setup & Fixtures: an idle surface in normal mode.
    let mut state = AppState::default();
    state.enter_normal_mode();

    // -- Exec: the burst folds, then the key.
    for i in 0..3 {
        let chunk = format!("chunk {i}");
        reduce_narration(&mut state, inference_chunk(chunk.clone(), chunk));
    }
    let changed = fold_key(&mut state, Key::Char('i'), None);

    // -- Check: the key still folds.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Insert);
    Ok(())
}

// row 15 — the lag warn does not stop the next key from folding.
#[test]
fn test_input_matrix_key_folds_after_stream_lag() -> Result<()> {
    // -- Setup & Fixtures: an idle surface in insert mode.
    let mut state = AppState::default();

    // -- Exec: the lag's own line, then the key. A lag is a log line, not a
    // fold: the loop warns and re-syncs at the tail, so no state change stands
    // between the warn and the key.
    reduce_narration(
        &mut state,
        system_message(NoticeKind::Warning, "tui-lag: stream lane lagged"),
    );
    let changed = fold_key(&mut state, Key::Esc, None);

    // -- Check: the key still folds.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    Ok(())
}
