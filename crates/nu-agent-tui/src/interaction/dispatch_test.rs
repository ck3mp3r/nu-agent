use crate::{
    interaction::{
        cancel::CancelController,
        dispatch::dispatch_terminal_event,
        input::{TerminalEvent, TerminalKey},
        reducer::ESC_ABORT_CONFIRM_STATUS,
    },
    state::{
        AgentPickerOption, AppState, CommandPaletteAction, InfoPanel, InputMode, McpServerState,
        McpServerUsabilityState, ModelPickerOption, UiPhase,
    },
};
use nu_agent_core::protocol::event::PermissionDecision;
use nu_agent_core::transcript::ir::Role;

fn open_permission_prompt(state: &mut AppState) {
    state.open_permission_prompt(crate::state::PermissionPrompt {
        request_id: "ask-0000000000000001".to_string(),
        matched_rule_identity: "nested:nu__run.command:*".to_string(),
        tool: "nu__run".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "tool[nu__run] args={\"command\":\"echo hi\"}".to_string(),
    });
}

#[test]
fn first_escape_in_busy_normal_sets_abort_pending_with_exact_status_text() {
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
    state.enter_normal_mode();

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
fn second_escape_in_abort_pending_after_busy_normal_toggles_cancel_requested() {
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
    state.enter_normal_mode();
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
    // Activate the prompt so the transcript entry is written
    let _ = state.take_next_prompt_for_execution();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.input.buffer, "s");
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role(), Role::User);
    assert_eq!(state.transcript_preview[0].text(), "f");
}

#[test]
fn submit_path_appends_prompt_and_keeps_input_editable() {
    let mut state = AppState::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        None,
    );
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input.locked);
    assert!(state.input.buffer.is_empty());
    let _ = state.take_next_prompt_for_execution();
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role(), Role::User);
    assert_eq!(state.transcript_preview[0].text(), "s");
}

#[test]
fn backspace_and_cursor_movement_edit_in_dispatch_path() {
    let mut state = AppState::new();

    for ch in ['a', 'b', 'c'] {
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char(ch)), None);
    }
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Left), None);
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Backspace),
        None,
    );

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
fn esc_in_busy_insert_mode_switches_to_normal_without_abort_side_effect() {
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

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input_mode, InputMode::Insert);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert!(!state.abort.pending);
    assert!(!cancel_controller.is_cancel_requested());
}

#[test]
fn jj_chord_in_busy_insert_mode_switches_to_normal_mode() {
    let mut state = AppState::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('w')),
        None,
    );
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input_mode, InputMode::Insert);

    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first);
    assert_eq!(state.input_mode, InputMode::Insert);

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(second);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn jk_chord_in_busy_insert_mode_switches_to_normal_mode() {
    let mut state = AppState::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('w')),
        None,
    );
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input_mode, InputMode::Insert);

    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first);
    assert_eq!(state.input_mode, InputMode::Insert);

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn busy_normal_mode_blocks_plain_typing_until_explicit_i() {
    let mut state = AppState::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('w')),
        None,
    );
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input_mode, InputMode::Insert);

    let esc = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(esc);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);

    let typed_while_normal = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!typed_while_normal);
    assert!(state.input.buffer.is_empty());
    assert_eq!(state.input_mode, InputMode::Normal);

    let enter_insert = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );
    assert!(enter_insert);
    assert_eq!(state.input_mode, InputMode::Insert);

    let typed_after_i = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(typed_after_i);
    assert_eq!(state.input.buffer, "x");
}

#[test]
fn busy_normal_mode_after_jk_chord_requires_i_before_typing() {
    let mut state = AppState::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('w')),
        None,
    );
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input_mode, InputMode::Insert);

    let first_j = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first_j);
    let second_k = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second_k);
    assert_eq!(state.input_mode, InputMode::Normal);

    let typed_while_normal = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!typed_while_normal);
    assert!(state.input.buffer.is_empty());

    let enter_insert = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );
    assert!(enter_insert);
    assert_eq!(state.input_mode, InputMode::Insert);

    let typed_after_i = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(typed_after_i);
    assert_eq!(state.input.buffer, "z");
}

#[test]
fn normal_mode_blocks_plain_typing_and_keeps_input_unchanged() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );

    assert!(!changed);
    assert!(state.input.buffer.is_empty());
    assert_eq!(state.input_mode, InputMode::Normal);
}

#[test]
fn normal_mode_hl_cycles_focus_between_panes() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('l')),
        None,
    );
    assert!(first);
    assert_eq!(state.pane_focus, crate::state::PaneFocus::Input);

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('h')),
        None,
    );
    assert!(second);
    assert_eq!(state.pane_focus, crate::state::PaneFocus::Transcript);
}

#[test]
fn normal_mode_tab_and_backtab_cycle_focus_between_transcript_and_input() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let tab = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert!(tab);
    assert_eq!(state.pane_focus, crate::state::PaneFocus::Input);

    let backtab =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::BackTab), None);
    assert!(backtab);
    assert_eq!(state.pane_focus, crate::state::PaneFocus::Transcript);
}

#[test]
fn permission_prompt_key_a_submits_allow_once() {
    let mut state = AppState::new();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('a')),
        None,
    );
    assert!(changed);
    assert!(!state.has_permission_prompt());

    let submission = state
        .take_next_permission_decision_submission()
        .expect("permission submission");
    assert_eq!(submission.decision, PermissionDecision::AllowOnce);
}

#[test]
fn permission_prompt_key_upper_a_submits_allow_always() {
    let mut state = AppState::new();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('A')),
        None,
    );
    assert!(changed);
    assert!(!state.has_permission_prompt());

    let submission = state
        .take_next_permission_decision_submission()
        .expect("permission submission");
    assert_eq!(submission.decision, PermissionDecision::AllowAlways);
}

#[test]
fn permission_prompt_key_d_submits_deny() {
    let mut state = AppState::new();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('d')),
        None,
    );
    assert!(changed);
    assert!(!state.has_permission_prompt());

    let submission = state
        .take_next_permission_decision_submission()
        .expect("permission submission");
    assert_eq!(submission.decision, PermissionDecision::Deny);
}

#[test]
fn permission_prompt_esc_submits_deny() {
    let mut state = AppState::new();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert!(!state.has_permission_prompt());

    let submission = state
        .take_next_permission_decision_submission()
        .expect("permission submission");
    assert_eq!(submission.decision, PermissionDecision::Deny);
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
fn normal_mode_z_is_noop() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    let z = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!z);
}

#[test]
fn insert_mode_alt_and_shift_enter_insert_newline_while_enter_submits() {
    let mut state = AppState::new();

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('h')),
        None,
    );
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::AltEnter), None);
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::ShiftEnter),
        None,
    );

    assert_eq!(state.input.buffer, "h\n\n");
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(state.transcript_preview.is_empty());

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    let _ = state.take_next_prompt_for_execution();
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].text(), "h");
}

#[test]
fn ctrl_p_opens_palette_and_second_ctrl_p_moves_selection_up() {
    let mut state = AppState::new();

    let opened = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(opened);
    assert!(state.command_palette_open);

    // Move down so there's somewhere to go up
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.command_palette_selection, 1);

    // Ctrl-P while palette open moves selection up (does not close)
    let moved_up =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(moved_up);
    assert!(state.command_palette_open);
    assert_eq!(state.command_palette_selection, 0);

    // Esc closes the palette
    let closed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(closed);
    assert!(!state.command_palette_open);
}

#[test]
fn escape_closes_palette_only_and_preserves_insert_mode() {
    let mut state = AppState::new();
    assert_eq!(state.input_mode, InputMode::Insert);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(state.command_palette_open);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert!(!state.command_palette_open);
    assert_eq!(state.input_mode, InputMode::Insert);
}

#[test]
fn palette_navigation_supports_arrows_and_ctrl_np_and_enter_routes_action() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.command_palette_selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(state.command_palette_selection, 0);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);
    assert_eq!(state.command_palette_selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Status));
    assert!(!state.command_palette_open);
}

#[test]
fn palette_selection_can_open_mcps_panel() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(
        state.command_palette_selected_action(),
        Some(CommandPaletteAction::Mcps)
    );

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Mcps));
    assert!(!state.command_palette_open);
}

#[test]
fn palette_selection_can_open_skills_panel() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs -> Skills
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(
        state.command_palette_selected_action(),
        Some(CommandPaletteAction::Skills)
    );

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Skills));
    assert!(!state.command_palette_open);
}

#[test]
fn command_palette_models_action_opens_inline_model_picker() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs -> Skills -> Models
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(
        state.command_palette_selected_action(),
        Some(CommandPaletteAction::Models)
    );

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(state.take_next_model_picker_launch_request());
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert!(!state.command_palette_open);
}

#[test]
fn models_slash_and_palette_share_same_action_handler() {
    let mut slash_state = AppState::new();
    for ch in "/models".chars() {
        let _ = dispatch_terminal_event(
            &mut slash_state,
            &TerminalEvent::Key(TerminalKey::Char(ch)),
            None,
        );
    }
    let _ = dispatch_terminal_event(
        &mut slash_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert!(slash_state.take_next_model_picker_launch_request());
    assert_eq!(slash_state.take_next_prompt_for_execution(), None);

    let mut palette_state = AppState::new();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );

    assert!(palette_state.take_next_model_picker_launch_request());
    assert_eq!(palette_state.take_next_prompt_for_execution(), None);
}

#[test]
fn palette_models_does_not_bypass_shared_models_action_path() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert!(!state.model_picker_open);
    assert!(!state.command_palette_open);
    assert!(state.take_next_model_picker_launch_request());
    assert_eq!(state.take_next_prompt_for_execution(), None);
}

#[test]
fn models_launcher_opens_picker_while_worker_active() {
    let mut state = AppState::new();
    state.enqueue_prompt("first".to_string());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("first".to_string())
    );

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert!(state.take_next_model_picker_launch_request());
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());
}

#[test]
fn models_slash_opens_picker_while_worker_active() {
    let mut state = AppState::new();
    state.enqueue_prompt("first".to_string());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("first".to_string())
    );

    for ch in "/models".chars() {
        let _ =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char(ch)), None);
    }

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert!(state.take_next_model_picker_launch_request());
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());
}

#[test]
fn model_picker_query_accepts_j_and_k_characters() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "jk-provider-a".to_string(),
            model: "jk-model-a".to_string(),
            identity: "jk-provider-a/jk-model-a".to_string(),
            display: "jk-provider-a / jk-model-a".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "jk-provider-b".to_string(),
            model: "jk-model-b".to_string(),
            identity: "jk-provider-b/jk-model-b".to_string(),
            display: "jk-provider-b / jk-model-b".to_string(),
            active: false,
        },
    ]);
    state.open_model_picker();

    let j_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    let k_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );

    assert!(j_changed);
    assert!(k_changed);
    assert_eq!(state.model_picker_query, "jk");
    assert_eq!(state.model_picker_filtered_options().len(), 2);
}

#[test]
fn model_picker_navigation_does_not_consume_query_jk_input() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "jk-provider-a".to_string(),
            model: "jk-model-a".to_string(),
            identity: "jk-provider-a/jk-model-a".to_string(),
            display: "jk-provider-a / jk-model-a".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "jk-provider-b".to_string(),
            model: "jk-model-b".to_string(),
            identity: "jk-provider-b/jk-model-b".to_string(),
            display: "jk-provider-b / jk-model-b".to_string(),
            active: false,
        },
        ModelPickerOption {
            provider: "jk-provider-c".to_string(),
            model: "jk-model-c".to_string(),
            identity: "jk-provider-c/jk-model-c".to_string(),
            display: "jk-provider-c / jk-model-c".to_string(),
            active: false,
        },
    ]);
    state.open_model_picker();

    let down_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_changed);
    assert_eq!(state.model_picker_selection, 1);

    let j_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(j_changed);
    assert_eq!(state.model_picker_query, "j");
    assert_eq!(state.model_picker_selection, 0);

    let k_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(k_changed);
    assert_eq!(state.model_picker_query, "jk");
    assert_eq!(state.model_picker_selection, 0);

    let down_again_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_again_changed);
    assert_eq!(state.model_picker_selection, 1);

    let up_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Up), None);
    assert!(up_changed);
    assert_eq!(state.model_picker_selection, 0);
}

#[test]
fn model_picker_ctrl_n_moves_to_next_item() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
        },
    ]);
    state.open_model_picker();

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.model_picker_selection, 1);
}

#[test]
fn query_picker_ctrl_n_moves_to_next_item_consistently() {
    let mut palette_state = AppState::new();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    assert!(palette_state.command_palette_open);

    let palette_changed = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlN),
        None,
    );
    assert!(palette_changed);
    assert_eq!(palette_state.command_palette_selection, 1);

    let mut model_state = AppState::new();
    model_state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
        },
    ]);
    model_state.open_model_picker();

    let model_changed = dispatch_terminal_event(
        &mut model_state,
        &TerminalEvent::Key(TerminalKey::CtrlN),
        None,
    );
    assert!(model_changed);
    assert_eq!(model_state.model_picker_selection, 1);
}

#[test]
fn esc_closes_mcps_panel_and_preserves_insert_mode() {
    let mut state = AppState::new();
    state.open_info_panel(InfoPanel::Mcps);
    assert_eq!(state.input_mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_eq!(state.info_panel, None);
    assert_eq!(state.input_mode, InputMode::Insert);
}

#[test]
fn mcps_panel_navigation_and_enter_toggle_updates_selected_server() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.mcp_panel_selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(
        state.mcp_servers[1].state,
        McpServerUsabilityState::Disabled,
        "enable is async; state remains disabled until runtime applies result"
    );

    let request = state
        .take_next_mcp_toggle_request()
        .expect("queued toggle request");
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);
}

#[test]
fn mcps_panel_supports_up_ctrl_p_and_space_toggle() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);
    state.mcp_panel_selection = 1;

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Up), None);
    assert_eq!(state.mcp_panel_selection, 0);

    // Ctrl-P moves selection up (wraps: 0 -> len-1 = 1)
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(state.mcp_panel_selection, 1);

    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char(' ')),
        None,
    );
    let request = state
        .take_next_mcp_toggle_request()
        .expect("queued toggle request");
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);
}

#[test]
fn palette_filters_with_non_prefix_query_before_enter_routes_help() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('h')),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('p')),
        None,
    );
    assert_eq!(
        state.command_palette_actions(),
        vec![crate::state::CommandPaletteAction::Help]
    );

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Help));
    assert!(!state.command_palette_open);
}

#[test]
fn escape_closes_info_panel_without_mode_regression() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Help));
    assert_eq!(state.input_mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_eq!(state.info_panel, None);
    assert_eq!(state.input_mode, InputMode::Insert);
}

#[test]
fn existing_insert_mode_jk_chord_still_switches_to_normal_outside_palette() {
    let mut state = AppState::new();
    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first);
    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input_mode, InputMode::Normal);
}

#[test]
fn inline_slash_suggestions_open_on_leading_slash() {
    let mut state = AppState::new();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('/')),
        None,
    );
    assert!(changed);
    assert!(state.inline_slash_open);
    assert!(!state.command_palette_open);
}

#[test]
fn inline_slash_enter_on_compact_triggers_compaction_path() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('/')),
        None,
    );

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert!(state.transcript_preview.is_empty());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("/compact".to_string())
    );
    assert!(!state.inline_slash_open);
    assert!(!state.command_palette_open);
}

#[test]
fn immediate_slash_commands_do_not_set_busy_or_spinner() {
    for command in ["/compact", "/mcp", "/help", "/status"] {
        let mut state = AppState::new();

        for ch in command.chars() {
            let changed = dispatch_terminal_event(
                &mut state,
                &TerminalEvent::Key(TerminalKey::Char(ch)),
                None,
            );
            assert!(changed);
        }

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
        assert!(changed);
        assert_eq!(state.phase, UiPhase::Idle);
        assert!(!state.is_active_cycle());
        assert_eq!(state.pending_prompt_count(), 0);
        assert!(state.prompt_items().is_empty());
        assert!(state.status_line != "Thinking...");
    }
}

#[test]
fn inline_slash_suggestions_close_when_prefix_removed() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('/')),
        None,
    );
    assert!(state.inline_slash_open);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Backspace),
        None,
    );
    assert!(changed);

    assert_eq!(state.input.buffer, "");
    assert!(!state.inline_slash_open);
    assert!(!state.command_palette_open);
}

#[test]
fn palette_escape_closes_without_panel_route_regression() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(state.command_palette_open);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert!(!state.command_palette_open);
    assert_eq!(state.info_panel, None);
}

// ---- agent picker dispatch tests ----

fn setup_agent_picker_open() -> AppState {
    let mut state = AppState::new();
    state.set_agent_picker_options(vec![
        AgentPickerOption {
            name: "alpha".into(),
            description: Some("Alpha agent".into()),
            display: "alpha — Alpha agent".into(),
            active: false,
            builtin: false,
        },
        AgentPickerOption {
            name: "beta".into(),
            description: None,
            display: "beta".into(),
            active: true,
            builtin: false,
        },
        AgentPickerOption {
            name: "gamma".into(),
            description: Some("Gamma agent".into()),
            display: "gamma — Gamma agent".into(),
            active: false,
            builtin: false,
        },
    ]);
    state.open_agent_picker();
    state
}

#[test]
fn agent_picker_open_insert_char_appends_to_query() {
    let mut state = setup_agent_picker_open();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('a')),
        None,
    );

    assert!(changed);
    assert_eq!(state.agent_picker_query, "a");
    assert!(state.agent_picker_open);
}

#[test]
fn agent_picker_open_esc_closes_picker() {
    let mut state = setup_agent_picker_open();

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    assert!(changed);
    assert!(!state.agent_picker_open);
    assert_eq!(state.agent_picker_query, "");
    assert_eq!(state.agent_picker_selection, 0);
}

#[test]
fn agent_picker_open_submit_queues_switch_request_and_closes() {
    let mut state = setup_agent_picker_open();

    // Move to beta (sorted: alpha=0, beta=1, gamma=2)
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert!(!state.agent_picker_open);
    assert_eq!(
        state.take_next_agent_switch_request(),
        Some("beta".to_string())
    );
}

#[test]
fn agent_picker_closed_actions_pass_through_normally() {
    let mut state = AppState::new();
    state.set_agent_picker_options(vec![AgentPickerOption {
        name: "alpha".into(),
        description: None,
        display: "alpha".into(),
        active: true,
        builtin: false,
    }]);
    // Picker is NOT open
    assert!(!state.agent_picker_open);

    // Char should go to input buffer
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(changed);
    assert_eq!(state.input.buffer, "x");
    assert!(!state.agent_picker_open);
}

#[test]
fn agent_slash_and_palette_share_same_action_handler() {
    // /agent slash command triggers agent picker launch
    let mut slash_state = AppState::new();
    for ch in "/agent".chars() {
        let _ = dispatch_terminal_event(
            &mut slash_state,
            &TerminalEvent::Key(TerminalKey::Char(ch)),
            None,
        );
    }
    let _ = dispatch_terminal_event(
        &mut slash_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert!(slash_state.take_next_agent_picker_launch_request());
    assert_eq!(slash_state.take_next_prompt_for_execution(), None);

    // Command palette Agents action triggers same path
    let mut palette_state = AppState::new();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    // Help -> Status -> MCPs -> Skills -> Models -> Agents
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    assert_eq!(
        palette_state.command_palette_selected_action(),
        Some(CommandPaletteAction::Agents)
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert!(palette_state.take_next_agent_picker_launch_request());
    assert_eq!(palette_state.take_next_prompt_for_execution(), None);
}

// ---- Tab cycling between built-in agents ----

#[test]
fn test_tab_cycles_agent_in_insert_mode() {
    let mut state = AppState::new();
    // Insert mode is default
    assert_eq!(state.input_mode, InputMode::Insert);
    // Set up 2+ builtin cycle names
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");
    // No modals open (default)
    assert!(!state.agent_picker_open);
    assert!(!state.model_picker_open);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    assert!(changed);
    // Tab should have queued a cycle request, resulting in Noop + force_changed
    let request = state.take_next_agent_switch_request();
    assert_eq!(request, Some("maker".to_string()));
}

#[test]
fn test_tab_does_not_cycle_in_normal_mode() {
    let mut state = AppState::new();
    state.enter_normal_mode();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    assert!(changed);
    // In normal mode, Tab maps to FocusPaneRight, NOT agent cycling
    // Verify no agent switch was queued
    assert_eq!(state.take_next_agent_switch_request(), None);
    // Focus should have cycled
    assert_eq!(state.pane_focus, crate::state::PaneFocus::Input);
}

#[test]
fn test_tab_does_not_cycle_when_no_builtins() {
    let mut state = AppState::new();
    assert_eq!(state.input_mode, InputMode::Insert);
    // Empty cycle names
    state.agent_cycle_names = Vec::new();

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    // Tab should pass through — no agent switch queued
    assert_eq!(state.take_next_agent_switch_request(), None);
    // In idle insert mode, Tab maps to CompleteForward which is a no-op in the reducer
    // (no slash menu open, no special handling), so changed depends on state diff
    let _ = changed; // outcome is not Noop+true since cycling didn't fire
}

// ---- Ctrl-N/Ctrl-P picker navigation, j/k feed query in command palette ----

#[test]
fn command_palette_j_feeds_query_not_navigation() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(state.command_palette_open);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    assert!(changed);
    assert_eq!(state.command_palette_query, "j");
    // Selection should not have changed (still at 0)
    assert_eq!(state.command_palette_selection, 0);
    assert!(state.command_palette_open);
}

#[test]
fn command_palette_k_feeds_query_not_navigation() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(state.command_palette_open);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );

    assert!(changed);
    assert_eq!(state.command_palette_query, "k");
    assert_eq!(state.command_palette_selection, 0);
    assert!(state.command_palette_open);
}

#[test]
fn command_palette_ctrl_n_moves_selection_down() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(state.command_palette_open);
    assert_eq!(state.command_palette_selection, 0);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.command_palette_selection, 1);
    assert!(state.command_palette_open);
}

#[test]
fn command_palette_ctrl_p_moves_selection_up() {
    let mut state = AppState::new();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(state.command_palette_open);

    // Move down first so we have somewhere to go up
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.command_palette_selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.command_palette_selection, 0);
    assert!(state.command_palette_open);
}

#[test]
fn model_picker_ctrl_p_moves_selection_up() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
        },
    ]);
    state.open_model_picker();

    // Move down first
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.model_picker_selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.model_picker_selection, 0);
    assert!(state.model_picker_open);
}

#[test]
fn agent_picker_ctrl_p_moves_selection_up() {
    let mut state = setup_agent_picker_open();

    // Move down first
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.agent_picker_selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.agent_picker_selection, 0);
    assert!(state.agent_picker_open);
}

#[test]
fn ctrl_p_with_no_modal_open_opens_command_palette() {
    let mut state = AppState::new();
    assert!(!state.command_palette_open);
    assert!(!state.model_picker_open);
    assert!(!state.agent_picker_open);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert!(state.command_palette_open);
}

// ---- Ctrl-N/Ctrl-P navigation in non-picker panels (Help, Status, Skills, MCPs) ----

#[test]
fn help_panel_ctrl_n_scrolls_down() {
    let mut state = AppState::new();
    state.open_info_panel(crate::state::InfoPanel::Help);
    assert_eq!(state.info_panel_scroll, 0);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.info_panel_scroll, 1);
}

#[test]
fn help_panel_ctrl_p_scrolls_up() {
    let mut state = AppState::new();
    state.open_info_panel(crate::state::InfoPanel::Help);
    state.info_panel_scroll = 3;

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.info_panel_scroll, 2);
}

#[test]
fn help_panel_j_is_noop() {
    let mut state = AppState::new();
    state.open_info_panel(crate::state::InfoPanel::Help);
    state.info_panel_scroll = 0;

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    // j is not a navigation key in the help panel — scroll must not change
    assert_eq!(state.info_panel_scroll, 0);
    // changed may be true or false depending on reducer noop handling;
    // the important assertion is that scroll did not increment
    let _ = changed;
}

#[test]
fn mcp_panel_ctrl_n_moves_selection_down() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);
    assert_eq!(state.mcp_panel_selection, 0);

    let _ =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert_eq!(state.mcp_panel_selection, 1);
}

#[test]
fn mcp_panel_ctrl_p_moves_selection_up() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);
    state.mcp_panel_selection = 1;

    let _ =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert_eq!(state.mcp_panel_selection, 0);
}

#[test]
fn mcp_panel_j_is_noop() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);
    assert_eq!(state.mcp_panel_selection, 0);

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    // j is no longer a navigation key in the MCPs panel — selection must not change
    assert_eq!(state.mcp_panel_selection, 0);
}
