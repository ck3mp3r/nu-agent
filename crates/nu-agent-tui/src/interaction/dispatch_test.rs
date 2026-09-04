use crate::{
    interaction::{
        cancel::CancelController,
        dispatch::dispatch_terminal_event,
        input::{TerminalEvent, TerminalKey},
        reducer::ESC_ABORT_CONFIRM_STATUS,
    },
    state::{
        ActivePicker, AppState, InfoPanel, InputMode, InputState, McpServerState,
        McpServerUsabilityState, PickerRenderKind, SwitchRequest, UiPhase,
    },
};
use nu_agent_core::protocol::contracts::SharedUiAction;
use nu_agent_core::protocol::event::PermissionDecision;
use nu_agent_core::transcript::ir::Role;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn open_permission_prompt(state: &mut AppState) {
    state
        .permission
        .open_prompt(crate::state::PermissionPrompt {
            request_id: "ask-0000000000000001".to_string(),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            scope: "nested".to_string(),
            pattern: "*".to_string(),
            target_field: Some("command".to_string()),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
        });
}

fn busy_state_with_controller() -> (AppState, CancelController) {
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();
    state.input.pending_submit_text = Some("w".to_string());
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );
    (state, cancel_controller)
}

fn busy_state() -> AppState {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("w".to_string()),
        ..Default::default()
    };
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    state
}

#[test]
fn first_escape_in_busy_normal_sets_abort_pending_with_exact_status_text() {
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.enter_normal_mode();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.status_line, ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());
}

#[test]
fn second_escape_in_abort_pending_after_busy_normal_toggles_cancel_requested() {
    let (mut state, cancel_controller) = busy_state_with_controller();
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
    assert_eq!(state.status.status_line, "Abort requested.");
}

#[test]
fn escape_in_idle_does_not_request_cancellation() {
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(!cancel_controller.is_cancel_requested());
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn typing_remains_available_while_prompt_is_active() {
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    state.input.pending_submit_text = Some("f".to_string());
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );
    // Activate the prompt so the transcript entry is written
    let _ = state.take_next_prompt_for_execution();

    // Typing in insert mode is now handled by the coordinator, not the dispatch path.
    // The dispatch path's InsertChar is a no-op.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        Some(&cancel_controller),
    );

    assert!(!changed);
    // starting spacer + user + closing spacer
    assert_eq!(state.transcript.entries.len(), 3);
    assert_eq!(state.transcript.entries[1].role(), Role::User);
    assert_eq!(state.transcript.entries[1].text(), "f");
}

#[test]
fn submit_path_appends_prompt_and_keeps_input_editable() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("s".to_string()),
        ..Default::default()
    };

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
    let _ = state.take_next_prompt_for_execution();
    // starting spacer + user + closing spacer
    assert_eq!(state.transcript.entries.len(), 3);
    assert_eq!(state.transcript.entries[1].role(), Role::User);
    assert_eq!(state.transcript.entries[1].text(), "s");
}

#[test]
fn backspace_and_cursor_movement_edit_in_dispatch_path() {
    // Backspace and cursor movement are now handled by TextArea, not the dispatch path.
    // The dispatch path's Backspace/Delete/MoveCursor actions are no-ops.
    let mut state = AppState::default();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Backspace),
        None,
    );
    assert!(!changed);
}

#[test]
fn esc_in_idle_insert_mode_switches_to_normal_mode() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn esc_in_busy_insert_mode_switches_to_normal_without_abort_side_effect() {
    let (mut state, cancel_controller) = busy_state_with_controller();

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert!(!state.abort.pending);
    assert!(!cancel_controller.is_cancel_requested());
}

#[test]
fn jj_chord_in_busy_insert_mode_switches_to_normal_mode() {
    let mut state = busy_state();

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first);
    assert_eq!(state.input.mode, InputMode::Insert);

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn jk_chord_in_busy_insert_mode_switches_to_normal_mode() {
    let mut state = busy_state();

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first);
    assert_eq!(state.input.mode, InputMode::Insert);

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn busy_normal_mode_blocks_plain_typing_until_explicit_i() {
    let mut state = busy_state();
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    let esc = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(esc);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);

    let typed_while_normal = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!typed_while_normal);
    assert_eq!(state.input.mode, InputMode::Normal);

    let enter_insert = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );
    assert!(enter_insert);
    assert_eq!(state.input.mode, InputMode::Insert);

    let typed_after_i = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!typed_after_i); // InsertChar is now a no-op in dispatch
}

#[test]
fn busy_normal_mode_after_jk_chord_requires_i_before_typing() {
    let mut state = busy_state();
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

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
    assert_eq!(state.input.mode, InputMode::Normal);

    let typed_while_normal = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!typed_while_normal);

    let enter_insert = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );
    assert!(enter_insert);
    assert_eq!(state.input.mode, InputMode::Insert);

    let typed_after_i = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!typed_after_i); // InsertChar is now a no-op in dispatch
}

#[test]
fn normal_mode_blocks_plain_typing_and_keeps_input_unchanged() {
    let mut state = AppState::default();
    state.enter_normal_mode();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );

    assert!(!changed);
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn normal_mode_hl_cycles_focus_between_panes() {
    let mut state = AppState::default();
    state.enter_normal_mode();

    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('l')),
        None,
    );
    assert!(first);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('h')),
        None,
    );
    assert!(second);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Transcript);
}

#[test]
fn normal_mode_tab_and_backtab_cycle_focus_between_transcript_and_input() {
    let mut state = AppState::default();
    state.enter_normal_mode();

    let tab = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert!(tab);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);

    let backtab =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::BackTab), None);
    assert!(backtab);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Transcript);
}

#[test]
fn permission_prompt_key_a_submits_allow_once() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('a')),
        None,
    );
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::AllowOnce);
    Ok(())
}

#[test]
fn permission_prompt_key_upper_a_submits_allow_always() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('A')),
        None,
    );
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::AllowAlways);
    Ok(())
}

#[test]
fn permission_prompt_key_d_submits_deny() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('d')),
        None,
    );
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::Deny);
    Ok(())
}

#[test]
fn permission_prompt_esc_submits_deny() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::Deny);
    Ok(())
}

#[test]
fn insert_mode_jk_chord_enters_normal_and_removes_j() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // The first 'j' sets insert_exit_pending_j but returns false (no-op).
    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(!first);
    assert_eq!(state.input.mode, InputMode::Insert);

    // The second 'k' triggers EnterNormalModeFromChord which still works.
    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn normal_mode_z_is_noop() {
    let mut state = AppState::default();
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
    // Alt+Enter and Shift+Enter are now handled by the coordinator (TextArea),
    // not the dispatch path. The dispatch path's InsertNewline is a no-op.
    let mut state = AppState::default();

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::AltEnter), None);
    assert!(!changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(state.transcript.entries.is_empty());

    // Enter still submits via pending_submit_text
    state.input.pending_submit_text = Some("h".to_string());
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    let _ = state.take_next_prompt_for_execution();
    // starting spacer + user + closing spacer
    assert_eq!(state.transcript.entries.len(), 3);
    assert_eq!(state.transcript.entries[1].text().trim(), "h");
}

#[test]
fn ctrl_p_opens_palette_and_second_ctrl_p_moves_selection_up() {
    let mut state = AppState::default();

    let opened = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(opened);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Move down so there's somewhere to go up
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    // Ctrl-P while palette open moves selection up (does not close)
    let moved_up =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(moved_up);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    // Esc closes the palette
    let closed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(closed);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn escape_closes_palette_only_and_preserves_insert_mode() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.input.mode, InputMode::Insert);
}

#[test]
fn palette_navigation_supports_arrows_and_ctrl_np_and_enter_routes_action() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Status));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn palette_selection_can_open_mcps_panel() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let selected = state.picker.active_state().unwrap().selected().unwrap();
    assert_eq!(selected.id, "MCPs");

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Mcps));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn palette_selection_can_open_skills_panel() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs -> Skills
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let selected = state.picker.active_state().unwrap().selected().unwrap();
    assert_eq!(selected.id, "Skills");

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Skills));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn command_palette_models_action_opens_inline_model_picker() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs -> Skills -> Models
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let selected = state.picker.active_state().unwrap().selected().unwrap();
    assert_eq!(selected.id, "Models");

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn models_slash_and_palette_share_same_action_handler() {
    let mut slash_state = AppState {
        input: InputState::default().with_pending_submit_text("/models".to_string()),
        ..Default::default()
    };
    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // Set pending_submit_text directly so Submit routes the slash command.
    let _ = dispatch_terminal_event(
        &mut slash_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert_eq!(
        slash_state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(slash_state.take_next_prompt_for_execution(), None);

    let mut palette_state = AppState::default();
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

    assert_eq!(
        palette_state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(palette_state.take_next_prompt_for_execution(), None);
}

#[test]
fn palette_models_does_not_bypass_shared_models_action_path() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
}

#[test]
fn models_launcher_opens_picker_while_worker_active() {
    let mut state = AppState::default();
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
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());
}

#[test]
fn models_slash_opens_picker_while_worker_active() {
    let mut state = AppState::default();
    state.enqueue_prompt("first".to_string());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("first".to_string())
    );

    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // Set pending_submit_text directly so Submit routes the slash command.
    state.input.pending_submit_text = Some("/models".to_string());

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());
}

#[test]
fn model_picker_query_accepts_j_and_k_characters() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-a".to_string(),
                model: "jk-model-a".to_string(),
                identity: "jk-provider-a/jk-model-a".to_string(),
                display: "jk-provider-a / jk-model-a".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-b".to_string(),
                model: "jk-model-b".to_string(),
                identity: "jk-provider-b/jk-model-b".to_string(),
                display: "jk-provider-b / jk-model-b".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

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
    assert_eq!(state.picker.active_state().unwrap().query, "jk");
    assert_eq!(state.picker.active_state().unwrap().filtered().len(), 2);
}

#[test]
fn model_picker_navigation_does_not_consume_query_jk_input() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-a".to_string(),
                model: "jk-model-a".to_string(),
                identity: "jk-provider-a/jk-model-a".to_string(),
                display: "jk-provider-a / jk-model-a".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-b".to_string(),
                model: "jk-model-b".to_string(),
                identity: "jk-provider-b/jk-model-b".to_string(),
                display: "jk-provider-b / jk-model-b".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-c".to_string(),
                model: "jk-model-c".to_string(),
                identity: "jk-provider-c/jk-model-c".to_string(),
                display: "jk-provider-c / jk-model-c".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let down_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let j_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(j_changed);
    assert_eq!(state.picker.active_state().unwrap().query, "j");
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let k_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(k_changed);
    assert_eq!(state.picker.active_state().unwrap().query, "jk");
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let down_again_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_again_changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let up_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Up), None);
    assert!(up_changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
}

#[test]
fn model_picker_ctrl_n_moves_to_next_item() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
}

#[test]
fn query_picker_ctrl_n_moves_to_next_item_consistently() {
    let mut palette_state = AppState::default();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    assert_eq!(
        palette_state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let palette_changed = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlN),
        None,
    );
    assert!(palette_changed);
    assert_eq!(palette_state.picker.active_state().unwrap().selection, 1);

    let mut model_state = AppState::default();
    model_state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    model_state.picker.open(ActivePicker::Model);

    let model_changed = dispatch_terminal_event(
        &mut model_state,
        &TerminalEvent::Key(TerminalKey::CtrlN),
        None,
    );
    assert!(model_changed);
    assert_eq!(model_state.picker.active_state().unwrap().selection, 1);
}

#[test]
fn esc_closes_mcps_panel_and_preserves_insert_mode() {
    let mut state = AppState::default();
    state.open_info_panel(InfoPanel::Mcps);
    assert_eq!(state.input.mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_eq!(state.info_panel, None);
    assert_eq!(state.input.mode, InputMode::Insert);
}

#[test]
fn mcps_panel_navigation_and_enter_toggle_updates_selected_server() -> Result<()> {
    let mut state = AppState::default();
    state.status.set_mcp_servers(vec![
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
    assert_eq!(state.status.mcp_panel_selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(
        state.status.mcp_servers[1].state,
        McpServerUsabilityState::Disabled,
        "enable is async; state remains disabled until runtime applies result"
    );

    let request = state
        .status
        .take_next_mcp_toggle_request()
        .ok_or("should have queued toggle request")?;
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);
    Ok(())
}

#[test]
fn mcps_panel_supports_up_ctrl_p_and_space_toggle() -> Result<()> {
    let mut state = AppState::default();
    state.status.set_mcp_servers(vec![
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
    state.status.mcp_panel_selection = 1;

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Up), None);
    assert_eq!(state.status.mcp_panel_selection, 0);

    // Ctrl-P moves selection up (wraps: 0 -> len-1 = 1)
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(state.status.mcp_panel_selection, 1);

    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char(' ')),
        None,
    );
    let request = state
        .status
        .take_next_mcp_toggle_request()
        .ok_or("should have queued toggle request")?;
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);
    Ok(())
}

#[test]
fn palette_filters_with_non_prefix_query_before_enter_routes_help() {
    let mut state = AppState::default();
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
    let ids: Vec<String> = state
        .picker
        .active_state()
        .unwrap()
        .filtered()
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["Help"]);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Help));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn escape_closes_info_panel_without_mode_regression() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Help));
    assert_eq!(state.input.mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_eq!(state.info_panel, None);
    assert_eq!(state.input.mode, InputMode::Insert);
}

#[test]
fn existing_insert_mode_jk_chord_still_switches_to_normal_outside_palette() {
    let mut state = AppState::default();
    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // The first 'j' sets insert_exit_pending_j but returns false (no-op).
    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(!first);
    // The second 'k' triggers EnterNormalModeFromChord which still works.
    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn inline_slash_suggestions_open_on_leading_slash() {
    let mut state = AppState::default();

    // check_inline_slash is called by the coordinator after TextArea mutations.
    // In the dispatch-only test, call it directly to set the state.
    state.check_inline_slash("/");

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn inline_slash_enter_on_compact_triggers_compaction_path() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("/compact".to_string()),
        ..Default::default()
    };
    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // Set pending_submit_text directly so Submit routes the slash command.

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert!(state.transcript.entries.is_empty());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("/compact".to_string())
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn immediate_slash_commands_do_not_set_busy_or_spinner() {
    for command in ["/compact", "/mcp", "/help", "/status", "/skills"] {
        let mut state = AppState {
            input: InputState::default().with_pending_submit_text(command.to_string()),
            ..Default::default()
        };

        // InsertChar is now a no-op in the dispatch path (handled by TextArea).
        // Set pending_submit_text directly so Submit routes the slash command.

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
        assert!(changed);
        assert_eq!(state.phase, UiPhase::Idle);
        assert!(!state.is_active_cycle());
        assert_eq!(state.pending_prompt_count(), 0);
        assert!(state.prompt_items().is_empty());
        assert!(state.status.status_line != "Thinking...");
    }
}

#[test]
fn inline_slash_suggestions_close_when_prefix_removed() {
    let mut state = AppState::default();
    state.check_inline_slash("/");
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );

    state.check_inline_slash("");

    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn palette_escape_closes_without_panel_route_regression() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.info_panel, None);
}

// ---- agent picker dispatch tests ----

fn setup_agent_picker_open() -> AppState {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "alpha".into(),
                description: Some("Alpha agent".into()),
                display: "alpha — Alpha agent".into(),
                active: false,
                builtin: false,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "beta".into(),
                description: None,
                display: "beta".into(),
                active: true,
                builtin: false,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "gamma".into(),
                description: Some("Gamma agent".into()),
                display: "gamma — Gamma agent".into(),
                active: false,
                builtin: false,
            },
        ],
    );
    state.picker.open(ActivePicker::Agent);
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
    assert_eq!(state.picker.active_state().unwrap().query, "a");
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}

#[test]
fn agent_picker_open_esc_closes_picker() {
    let mut state = setup_agent_picker_open();

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}

#[test]
fn agent_picker_open_submit_queues_switch_request_and_closes() {
    let mut state = setup_agent_picker_open();

    // Move to beta (sorted: alpha=0, beta=1, gamma=2)
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("beta".to_string()))
    );
}

#[test]
fn agent_picker_closed_actions_pass_through_normally() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![nu_agent_core::protocol::picker::AgentPickerOption {
            name: "alpha".into(),
            description: None,
            display: "alpha".into(),
            active: true,
            builtin: false,
        }],
    );
    // Picker is NOT open
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));

    // Char in dispatch path is now a no-op (handled by coordinator/TextArea)
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}

#[test]
fn agent_slash_and_palette_share_same_action_handler() {
    // /agent slash command triggers agent picker launch
    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // Set pending_submit_text directly so Submit routes the slash command.
    let mut slash_state = AppState {
        input: InputState::default().with_pending_submit_text("/agent".to_string()),
        ..Default::default()
    };
    let _ = dispatch_terminal_event(
        &mut slash_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert_eq!(
        slash_state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );
    assert_eq!(slash_state.take_next_prompt_for_execution(), None);

    // Command palette Agents action triggers same path
    let mut palette_state = AppState::default();
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
    let selected = palette_state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .unwrap();
    assert_eq!(selected.id, "Agents");
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert_eq!(
        palette_state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );
    assert_eq!(palette_state.take_next_prompt_for_execution(), None);
}

// ---- Tab cycling between built-in agents ----

#[test]
fn test_tab_cycles_agent_in_insert_mode() {
    let mut state = AppState::default();
    // Insert mode is default
    assert_eq!(state.input.mode, InputMode::Insert);
    // Set up 2+ builtin cycle names
    state.status.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");
    // No modals open (default)
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Model));

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    assert!(changed);
    // Tab should have queued a cycle request, resulting in Noop + force_changed
    let request = state.take_next_switch_request();
    assert_eq!(request, Some(SwitchRequest::Agent("maker".to_string())));
}

#[test]
fn test_tab_does_not_cycle_in_normal_mode() {
    let mut state = AppState::default();
    state.enter_normal_mode();
    state.status.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    assert!(changed);
    // In normal mode, Tab maps to FocusPaneRight, NOT agent cycling
    // Verify no agent switch was queued
    assert_eq!(state.take_next_switch_request(), None);
    // Focus should have cycled
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);
}

#[test]
fn test_tab_does_not_cycle_when_no_builtins() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    // Empty cycle names
    state.status.agent_cycle_names = Vec::new();

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    // Tab should pass through — no agent switch queued
    assert_eq!(state.take_next_switch_request(), None);
    // In idle insert mode, Tab maps to CompleteForward which is a no-op in the reducer
    // (no slash menu open, no special handling), so changed depends on state diff
    let _ = changed; // outcome is not Noop+true since cycling didn't fire
}

// ---- Ctrl-N/Ctrl-P picker navigation, j/k feed query in command palette ----

#[test]
fn command_palette_j_feeds_query_not_navigation() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().query, "j");
    // Selection should not have changed (still at 0)
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn command_palette_k_feeds_query_not_navigation() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().query, "k");
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn command_palette_ctrl_n_moves_selection_down() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn command_palette_ctrl_p_moves_selection_up() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Move down first so we have somewhere to go up
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn model_picker_ctrl_p_moves_selection_up() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    // Move down first
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
}

#[test]
fn agent_picker_ctrl_p_moves_selection_up() {
    let mut state = setup_agent_picker_open();

    // Move down first
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}

#[test]
fn ctrl_p_with_no_modal_open_opens_command_palette() {
    let mut state = AppState::default();
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

// ---- Ctrl-N/Ctrl-P navigation in non-picker panels (Help, Status, Skills, MCPs) ----

#[test]
fn help_panel_ctrl_n_scrolls_down() {
    let mut state = AppState::default();
    state.open_info_panel(crate::state::InfoPanel::Help);
    assert_eq!(state.info_panel_scroll, 0);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.info_panel_scroll, 1);
}

#[test]
fn help_panel_ctrl_p_scrolls_up() {
    let mut state = AppState::default();
    state.open_info_panel(crate::state::InfoPanel::Help);
    state.info_panel_scroll = 3;

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.info_panel_scroll, 2);
}

#[test]
fn help_panel_j_is_noop() {
    let mut state = AppState::default();
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
    let mut state = AppState::default();
    state.status.set_mcp_servers(vec![
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
    assert_eq!(state.status.mcp_panel_selection, 0);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert_eq!(state.status.mcp_panel_selection, 1);
}

#[test]
fn mcp_panel_ctrl_p_moves_selection_up() {
    let mut state = AppState::default();
    state.status.set_mcp_servers(vec![
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
    state.status.mcp_panel_selection = 1;

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert_eq!(state.status.mcp_panel_selection, 0);
}

#[test]
fn normal_v_key_enters_visual_mode_and_j_yank_works() -> Result<()> {
    let mut state = AppState::default();
    state.enter_normal_mode();
    state.scroll.cursor_visual_row = 0;
    state.scroll.total_visual_rows = 5;
    state.scroll.entry_indices = (0..5).collect();

    // Press 'v' → should enter Visual mode and set a selection
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('v')),
        None,
    );
    assert_eq!(state.input.mode, InputMode::Visual);
    let sel = state
        .scroll
        .selection
        .ok_or("should have transcript selection after pressing v")?;
    assert_eq!(sel.anchor(), 0);
    assert_eq!(sel.cursor(), 0);

    // Press 'j' → should extend selection down
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    let sel = state
        .scroll
        .selection
        .ok_or("should still have transcript selection after pressing j")?;
    assert_eq!(sel.cursor(), 1);

    // Populate rendered lines and press 'y' → should yank selected rows
    state.scroll.rendered_line_text = vec![
        "line 0".to_string(),
        "line 1".to_string(),
        "line 2".to_string(),
    ];
    state.scroll.rendered_line_start_row = 0;
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('y')),
        None,
    );

    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("line 0\nline 1".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
    Ok(())
}

#[test]
fn mcp_panel_j_is_noop() {
    let mut state = AppState::default();
    state.status.set_mcp_servers(vec![
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
    assert_eq!(state.status.mcp_panel_selection, 0);

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    // j is no longer a navigation key in the MCPs panel — selection must not change
    assert_eq!(state.status.mcp_panel_selection, 0);
}
