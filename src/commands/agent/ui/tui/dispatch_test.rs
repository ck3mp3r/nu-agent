use crate::commands::agent::ui::tui::{
    cancel::CancelController,
    dispatch::dispatch_terminal_event,
    input::{TerminalEvent, TerminalKey},
    reducer::ESC_ABORT_CONFIRM_STATUS,
    state::{AppState, InputMode, TranscriptRole, UiPhase},
};

#[test]
fn first_escape_in_busy_sets_abort_pending_with_exact_status_text() {
    let mut state = AppState::new();
    let cancel_controller = CancelController::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('w')),
        Some(&cancel_controller),
    );
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status_line, ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());
}

#[test]
fn second_escape_in_abort_pending_toggles_cancel_requested() {
    let mut state = AppState::new();
    let cancel_controller = CancelController::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('w')),
        Some(&cancel_controller),
    );
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert_eq!(state.status_line, "Abort requested.");
}

#[test]
fn escape_in_idle_does_not_request_cancellation() {
    let mut state = AppState::new();
    let cancel_controller = CancelController::new();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(!cancel_controller.is_cancel_requested());
    assert_eq!(state.input_mode, InputMode::Normal);
}

#[test]
fn typing_remains_available_while_prompt_is_active() {
    let mut state = AppState::new();
    let cancel_controller = CancelController::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('f')),
        Some(&cancel_controller),
    );
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.input.buffer, "s");
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role, TranscriptRole::User);
    assert_eq!(state.transcript_preview[0].text, "f");
}

#[test]
fn submit_path_appends_prompt_and_keeps_input_editable() {
    let mut state = AppState::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        None,
    );
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input.locked);
    assert!(state.input.buffer.is_empty());
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role, TranscriptRole::User);
    assert_eq!(state.transcript_preview[0].text, "s");
}

#[test]
fn backspace_and_cursor_movement_edit_in_dispatch_path() {
    let mut state = AppState::new();

    for ch in ['a', 'b', 'c'] {
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char(ch)), None);
    }
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Left), None);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Backspace), None);

    assert_eq!(state.input.buffer, "ac");
    assert_eq!(state.input.cursor, 1);

    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Delete), None);
    assert_eq!(state.input.buffer, "a");
    assert_eq!(state.input.cursor, 1);
}

#[test]
fn esc_in_idle_insert_mode_switches_to_normal_mode() {
    let mut state = AppState::new();
    assert_eq!(state.input_mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    assert!(changed);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn normal_mode_jk_scroll_transcript_without_editing_input() {
    let mut state = AppState::new();
    state.enter_normal_mode();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.scroll_transcript_page_up(2);
    let before = state.transcript_scroll_lines_from_bottom;

    let changed_down =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('j')), None);
    assert!(changed_down);
    assert!(state.transcript_scroll_lines_from_bottom <= before);

    let changed_up =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('k')), None);
    assert!(changed_up);
    assert!(!state.transcript_follow_tail);
    assert!(state.input.buffer.is_empty());
}

#[test]
fn normal_mode_blocks_plain_typing_and_keeps_input_unchanged() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('x')), None);

    assert!(!changed);
    assert!(state.input.buffer.is_empty());
    assert_eq!(state.input_mode, InputMode::Normal);
}

#[test]
fn normal_mode_hl_cycles_focus_between_panes() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let first = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('l')), None);
    assert!(first);
    assert_eq!(state.pane_focus, crate::commands::agent::ui::tui::state::PaneFocus::Input);

    let second = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('h')), None);
    assert!(second);
    assert_eq!(state.pane_focus, crate::commands::agent::ui::tui::state::PaneFocus::Transcript);
}

#[test]
fn normal_mode_tab_and_backtab_cycle_focus_between_transcript_and_input() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let tab = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert!(tab);
    assert_eq!(state.pane_focus, crate::commands::agent::ui::tui::state::PaneFocus::Input);

    let backtab =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::BackTab), None);
    assert!(backtab);
    assert_eq!(state.pane_focus, crate::commands::agent::ui::tui::state::PaneFocus::Transcript);
}

#[test]
fn normal_mode_gg_and_g_scroll_to_top_and_bottom() {
    let mut state = AppState::new();
    state.enter_normal_mode();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.scroll_transcript_page_up(5);

    let g1 = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('g')), None);
    assert!(!g1);

    let g2 = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('g')), None);
    assert!(g2);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 7);

    let k = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('k')), None);
    assert!(!k);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 7);

    let j = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('j')), None);
    assert!(j);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 7);

    let g_cap =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('G')), None);
    assert!(g_cap);
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
}

#[test]
fn insert_mode_jk_chord_enters_normal_and_removes_j() {
    let mut state = AppState::new();
    assert_eq!(state.input_mode, InputMode::Insert);

    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first);
    assert_eq!(state.input.buffer, "j");
    assert_eq!(state.input_mode, InputMode::Insert);

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert!(state.input.buffer.is_empty());
}

#[test]
fn normal_mode_v_enters_visual_and_yanks_selection_back_to_normal() {
    let mut state = AppState::new();
    state.push_transcript_line(TranscriptRole::Assistant, "l1");
    state.push_transcript_line(TranscriptRole::Assistant, "l2");
    state.enter_normal_mode();

    let v = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('v')), None);
    assert!(v);
    assert_eq!(state.input_mode, InputMode::Visual);

    let y = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('y')), None);
    assert!(y);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert!(state.take_clipboard_request().is_some());
}

#[test]
fn v_from_input_focus_is_noop_with_feedback() {
    let mut state = AppState::new();
    state.enter_normal_mode();
    state.focus_next_pane();
    assert_eq!(state.pane_focus, crate::commands::agent::ui::tui::state::PaneFocus::Input);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('v')), None);

    assert!(changed);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(
        state.status_line,
        "Visual mode requires transcript focus (Tab/h/l)."
    );
}

#[test]
fn gg_then_v_and_g_then_v_anchor_from_current_transcript_cursor() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.enter_normal_mode();

    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('g')), None);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('g')), None);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('v')), None);
    assert_eq!(state.visual_anchor_index(), Some(0));
    assert_eq!(state.visual_cursor_index(), Some(0));
    assert_eq!(state.selected_transcript_range(), Some((0, 0)));

    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('G')), None);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('v')), None);
    assert_eq!(state.visual_anchor_index(), Some(9));
    assert_eq!(state.visual_cursor_index(), Some(9));
    assert_eq!(state.selected_transcript_range(), Some((9, 9)));
}

#[test]
fn normal_mode_z_is_noop() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let z = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('z')), None);
    assert!(!z);
}

#[test]
fn g_then_k_detaches_follow_tail_immediately_in_normal_mode() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(5);
    for i in 0..20 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.enter_normal_mode();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('G')), None);
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(19));

    let k = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('k')), None);
    assert!(k);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(18));
}

#[test]
fn g_then_ctrl_u_detaches_follow_tail_immediately_in_normal_mode() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(5);
    for i in 0..20 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.enter_normal_mode();

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('G')), None);
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(19));

    let ctrl_u = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlU), None);
    assert!(ctrl_u);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(11));
}

#[test]
fn g_then_page_up_detaches_follow_tail_immediately_in_normal_mode() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(5);
    for i in 0..20 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.enter_normal_mode();

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('G')), None);
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(19));

    let pgup = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::PageUp), None);
    assert!(pgup);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(11));
}

#[test]
fn g_then_k_detaches_follow_tail_immediately_in_visual_mode() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(5);
    for i in 0..20 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.enter_normal_mode();

    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('G')), None);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('v')), None);
    assert_eq!(state.input_mode, InputMode::Visual);
    assert!(state.transcript_follow_tail);
    assert_eq!(state.visual_cursor_index(), Some(19));

    let k = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('k')), None);
    assert!(k);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.visual_cursor_index(), Some(18));
}

#[test]
fn g_then_ctrl_u_detaches_follow_tail_immediately_in_visual_mode() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(5);
    for i in 0..20 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.enter_normal_mode();

    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('G')), None);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char('v')), None);
    assert_eq!(state.input_mode, InputMode::Visual);
    assert!(state.transcript_follow_tail);
    assert_eq!(state.visual_cursor_index(), Some(19));

    let ctrl_u = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlU), None);
    assert!(ctrl_u);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.visual_cursor_index(), Some(11));
}
