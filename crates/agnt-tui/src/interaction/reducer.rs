use crate::{
    interaction::{cancel::CancelController, dispatch::KeyLens},
    state::{
        ActivePicker, AppState, CommandPaletteAction, InputMode, PermissionPrompt, PickerOption,
        PickerPayload, PickerRenderKind, ScrollAction, SubmitAction, SwitchRequest, UiPhase,
    },
};
use agnt_core::protocol::contracts::SharedUiAction;
use agnt_core::protocol::event::{PermissionDecision, PermissionRequestContext};
use agnt_core::protocol::events::{Key, SystemEvent, SystemKind};
use agnt_core::protocol::slash::{SlashParseResult, extract_session_id, parse_slash_command};
use agnt_core::transcript::entry::{NoticeEntry, TranscriptEntry};
use agnt_core::transcript::ir::NoticeKind;

pub(crate) const ESC_ABORT_CONFIRM_STATUS: &str = "Esc again to cancel";

pub(crate) const PERMISSION_TIMEOUT_STATUS: &str = "permission request timed out — denied";

pub(crate) const VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS: &str =
    "Visual mode requires transcript focus (Tab/h/l).";

pub fn reduce(
    state: &mut AppState,
    event: SystemEvent,
    cancel_controller: Option<&CancelController>,
) -> bool {
    // One bookkeeping shift per event: the store counts the blocks it dropped,
    // so every push inside `event` is accounted for without threading an
    // evicted out-param through the domain reducers.
    let evicted_before = state.transcript.evicted_total();
    let changed = reduce_event(state, event, cancel_controller);
    let evicted = state
        .transcript
        .evicted_total()
        .saturating_sub(evicted_before);
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}

fn reduce_event(
    state: &mut AppState,
    event: SystemEvent,
    cancel_controller: Option<&CancelController>,
) -> bool {
    match event {
        SystemEvent::KeyPressed { key } => reduce_key(state, key, cancel_controller),
        // A resize performs no state mutation; the coordinator owns the layout.
        SystemEvent::WheelScrolled { up, column, row } => {
            reduce_wheel_event(state, up, column, row)
        }
        SystemEvent::Resized { .. } => false,
        // The card is the only writer of a decision: the reducer resolves the
        // request id from the open prompt, so the event carries no id.
        SystemEvent::PermissionDecided { decision } => reduce_permission_decided(state, decision),
        SystemEvent::PromptSubmitted { text } => handle_submit(state, text),
        SystemEvent::InferenceStarted => {
            state.llm.started(
                &mut state.transcript,
                &mut state.status,
                &mut state.phase,
                &mut state.input_locked,
            );
            state.ensure_invariants();
            true
        }
        SystemEvent::InferenceChunk { aggregated, .. } => {
            state
                .llm
                .chunk(&mut state.transcript, &mut state.scroll, &aggregated)
        }
        SystemEvent::InferenceCompleted { usage } => state.llm.completed(&mut state.status, &usage),
        SystemEvent::InferenceStopped { reason } => state.llm.stopped(
            &mut state.transcript,
            &mut state.status,
            &mut state.scroll,
            &reason,
        ),
        SystemEvent::ToolStarted { call } => state.tool.start_call(&mut state.transcript, &call),
        SystemEvent::PreviewRequested { call, display } => {
            let display = crate::state::preview_to_display(&display);
            state
                .tool
                .set_tool_preview(&mut state.transcript, &call.id, &display)
        }
        SystemEvent::ToolCompleted { call, outcome } => {
            state
                .tool
                .complete_call(&mut state.transcript, &call, &outcome)
        }
        SystemEvent::PermissionRequested {
            request_id,
            context,
        } => {
            apply_permission_request_display(state, &context);
            state.permission.open_prompt(PermissionPrompt::from_context(
                request_id.as_str(),
                &context,
            ));
            state.scroll.scroll_transcript_to_bottom();
            state.ensure_invariants();
            true
        }
        SystemEvent::PermissionDecisionTimedOut { request_id } => {
            let changed = state.permission.close_prompt_if(request_id.as_str());
            if changed {
                state.status.message.set_message(PERMISSION_TIMEOUT_STATUS);
            }
            changed
        }
        // The echo events of the permission lifecycle are not rendered by the
        // TUI: the card is the only surface, and it closes on submission.
        SystemEvent::PermissionDecisionSubmitted { .. }
        | SystemEvent::PermissionDecisionIgnored { .. }
        | SystemEvent::PermissionRequestCancelled { .. }
        | SystemEvent::PermissionAutoResolved { .. } => false,
        SystemEvent::SystemMessage { kind, text } => match kind {
            SystemKind::Warning => {
                state.status.message.set_warning(text);
                true
            }
            SystemKind::TurnError => handle_turn_error(state, &text),
        },
        SystemEvent::CompactionRequested { .. } => false,
        SystemEvent::CompactionStarted { source } => {
            state.compaction.started(&mut state.transcript, &source)
        }
        SystemEvent::CompactionSummaryChunk {
            source, aggregated, ..
        } => state.compaction.summary_chunk(
            &mut state.transcript,
            &mut state.scroll,
            &source,
            &aggregated,
        ),
        SystemEvent::CompactionCompleted {
            source,
            summary_body,
            ..
        } => state.compaction.completed(
            &mut state.transcript,
            &mut state.status,
            &source,
            &summary_body,
        ),
        SystemEvent::CompactionFailed { source, message } => {
            state
                .compaction
                .failed(&mut state.transcript, &mut state.status, &source, &message)
        }
        SystemEvent::TurnCompleted { .. } => {
            state.finalize_cycle();
            // A cancel can end the turn while a card is still up (A2A task
            // cancel, Ctrl-C quit+cancel). The request is gone by then, so the
            // card must not outlive the turn.
            state.permission.close_prompt();
            state.turn.finalize(
                &mut state.transcript,
                &mut state.status,
                &mut state.input_locked,
            )
        }
        SystemEvent::Tick => true,
    }
}

// region:    --- Key arms

/// Fold one key: the input-boundary lens runs first (open panel, open picker,
/// the permission card, the agent chord); what it leaves behind is the vim
/// keymap, the insert-exit chords, and the pending `gg` key.
fn reduce_key(
    state: &mut AppState,
    key: Key,
    cancel_controller: Option<&CancelController>,
) -> bool {
    // The input lock drops editing keys while a turn runs. The card's own keys
    // stay reachable, so a decision can be taken while the input is locked.
    let locked = state.input_locked && !state.permission.has_prompt();
    if locked && edits_input(key) {
        return false;
    }

    match crate::interaction::dispatch::rewrite_action(state, key) {
        KeyLens::Consumed(changed) => changed,
        KeyLens::Scroll(action) => state.scroll.reduce_scroll_action(action),
        KeyLens::PickerSubmit(submit) => handle_picker_submit(state, submit),
        KeyLens::Event(event) => reduce_key_event(state, *event, cancel_controller),
        KeyLens::Quit => handle_quit(state, cancel_controller),
        KeyLens::Escape => handle_escape(state),
        KeyLens::Pass => {
            // Busy-mode Esc escalation: AbortPending escalates Esc to the
            // cancel confirmation. It couples the orchestrator phase/abort
            // state with the reducer, so it sits above the keymap and below
            // the lens (an open panel or picker keeps its own Esc).
            if state.phase != UiPhase::Idle
                && key == Key::Esc
                && state.input.mode != InputMode::Insert
            {
                return reduce_busy_escape(state, cancel_controller);
            }
            reduce_keymap(state, key, cancel_controller)
        }
    }
}

fn reduce_key_event(
    state: &mut AppState,
    event: SystemEvent,
    cancel_controller: Option<&CancelController>,
) -> bool {
    match event {
        SystemEvent::PermissionDecided { decision } => reduce_permission_decided(state, decision),
        other => reduce_event(state, other, cancel_controller),
    }
}

fn reduce_permission_decided(state: &mut AppState, decision: PermissionDecision) -> bool {
    if state.permission.submit_decision(decision) {
        state.ensure_invariants();
    }
    true
}

/// The keys the input lock drops: everything that would edit the draft or
/// submit it. Every other key stays live (navigation, pickers, quit).
fn edits_input(key: Key) -> bool {
    matches!(
        key,
        Key::Char(_) | Key::Enter | Key::AltEnter | Key::ShiftEnter | Key::Backspace | Key::Delete
    )
}

fn reduce_keymap(
    state: &mut AppState,
    key: Key,
    cancel_controller: Option<&CancelController>,
) -> bool {
    let idle = state.phase == UiPhase::Idle;
    match state.input.mode {
        InputMode::Insert => reduce_insert_key(state, key, idle, cancel_controller),
        InputMode::Normal => reduce_normal_key(state, key, idle, cancel_controller),
        InputMode::Visual => reduce_visual_key(state, key, idle, cancel_controller),
    }
}

/// Insert mode: j/k exit chords (the second key within the window leaves insert
/// mode), Esc leaves insert mode, everything else falls through.
fn reduce_insert_key(
    state: &mut AppState,
    key: Key,
    idle: bool,
    cancel_controller: Option<&CancelController>,
) -> bool {
    match key {
        Key::Esc => reduce_insert_escape(state, idle),
        Key::Char('j') | Key::Char('k') => reduce_insert_chord(state, key, idle),
        other => {
            state.input.clear_insert_exit_pending_j();
            state.input.clear_normal_pending_key();
            reduce_common_key(state, other, cancel_controller)
        }
    }
}

fn reduce_insert_escape(state: &mut AppState, idle: bool) -> bool {
    state.input.clear_insert_exit_pending_j();
    state.input.clear_normal_pending_key();
    if idle {
        return handle_escape(state);
    }
    // Busy: Esc drops to normal mode first, then the abort-confirmation
    // coupling arms the cancel confirmation.
    state.enter_normal_mode();
    let _ = handle_escape(state);
    true
}

/// The insert-exit chord. Arming it is an input-state change, so a busy turn
/// repaints; while idle the armed key reports no change, exactly as the lens
/// did.
fn reduce_insert_chord(state: &mut AppState, key: Key, idle: bool) -> bool {
    let Key::Char(ch) = key else {
        return false;
    };
    if state.input.insert_exit_pending_j() {
        state.input.clear_insert_exit_pending_j();
        if idle {
            return handle_enter_normal_mode_from_chord(state);
        }
        state.enter_normal_mode();
        return true;
    }
    if ch == 'j' {
        state.input.set_insert_exit_pending_j();
    } else {
        state.input.clear_insert_exit_pending_j();
    }
    !idle
}

/// Normal mode: the vim navigation keymap, the pending `gg` key, and the mode
/// transitions. While busy the keymap still applies, but `i` re-enters insert
/// mode directly instead of through the idle gate.
fn reduce_normal_key(
    state: &mut AppState,
    key: Key,
    idle: bool,
    cancel_controller: Option<&CancelController>,
) -> bool {
    state.input.clear_insert_exit_pending_j();
    let select = false;
    match key {
        Key::Esc => {
            state.input.clear_normal_pending_key();
            handle_escape(state)
        }
        Key::Char('i') => {
            state.input.clear_normal_pending_key();
            if idle {
                handle_enter_insert_mode(state)
            } else {
                state.enter_insert_mode();
                true
            }
        }
        Key::Char('v') => {
            state.input.clear_normal_pending_key();
            handle_enter_visual_mode(state)
        }
        Key::Char('j') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineDown { select })
        }
        Key::Char('k') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineUp { select })
        }
        Key::Char('h') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneLeft)
        }
        Key::Char('l') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneRight)
        }
        Key::Tab => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneRight)
        }
        Key::BackTab => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneLeft)
        }
        Key::Char('g') => reduce_pending_g(state),
        Key::Char('G') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::ToBottom { select })
        }
        Key::PageUp | Key::CtrlU => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageUp { select })
        }
        Key::PageDown | Key::CtrlD => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageDown { select })
        }
        Key::Char(_) | Key::AltEnter | Key::ShiftEnter => {
            state.input.clear_normal_pending_key();
            false
        }
        other => {
            state.input.clear_normal_pending_key();
            reduce_common_key(state, other, cancel_controller)
        }
    }
}

/// Visual mode selects as it moves. While idle it owns a closed keymap — a key
/// it does not name does nothing; while busy the keys pass through to the
/// common arm, exactly as the lens did.
fn reduce_visual_key(
    state: &mut AppState,
    key: Key,
    idle: bool,
    cancel_controller: Option<&CancelController>,
) -> bool {
    if !idle {
        state.input.clear_insert_exit_pending_j();
        state.input.clear_normal_pending_key();
        return reduce_common_key(state, key, cancel_controller);
    }
    let select = true;
    match key {
        Key::CtrlP => {
            state.input.clear_normal_pending_key();
            handle_toggle_command_palette(state)
        }
        Key::Char('j') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineDown { select })
        }
        Key::Char('k') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineUp { select })
        }
        Key::Char('g') => reduce_pending_g(state),
        Key::Char('G') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::ToBottom { select })
        }
        Key::Char('y') => {
            state.input.clear_normal_pending_key();
            handle_yank_selection(state)
        }
        Key::Esc => {
            state.input.clear_normal_pending_key();
            handle_escape(state)
        }
        Key::PageUp | Key::CtrlU => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageUp { select })
        }
        Key::PageDown | Key::CtrlD => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageDown { select })
        }
        _ => {
            state.input.clear_normal_pending_key();
            false
        }
    }
}

/// The pending `gg` key: the first `g` arms it, the second one jumps to the top
/// of the transcript.
fn reduce_pending_g(state: &mut AppState) -> bool {
    let select = state.input.mode == InputMode::Visual;
    if state.input.take_normal_pending_key_if('g') {
        state
            .scroll
            .reduce_scroll_action(ScrollAction::ToTop { select })
    } else {
        state.input.arm_normal_pending_key('g');
        false
    }
}

/// The keys every mode shares: submit, quit, the palette, escape, and paging.
fn reduce_common_key(
    state: &mut AppState,
    key: Key,
    cancel_controller: Option<&CancelController>,
) -> bool {
    let select = state.input.mode == InputMode::Visual;
    match key {
        Key::Enter => {
            let text = state.input.pending_submit_text.take().unwrap_or_default();
            handle_submit(state, text)
        }
        Key::CtrlC => handle_quit(state, cancel_controller),
        Key::CtrlP => handle_toggle_command_palette(state),
        Key::Esc => handle_escape(state),
        Key::PageUp | Key::CtrlU => state
            .scroll
            .reduce_scroll_action(ScrollAction::PageUp { select }),
        Key::PageDown | Key::CtrlD => state
            .scroll
            .reduce_scroll_action(ScrollAction::PageDown { select }),
        // Editing keys are the coordinator's TextArea; at the reducer boundary
        // they are no-ops. History and query cycling are coordinator-side too.
        _ => false,
    }
}

/// The busy-mode Esc escalation: AbortPending confirms, everything else arms.
fn reduce_busy_escape(state: &mut AppState, cancel_controller: Option<&CancelController>) -> bool {
    state.input.clear_normal_pending_key();
    state.input.clear_insert_exit_pending_j();
    if state.phase == UiPhase::AbortPending && state.abort.pending {
        return handle_escape_confirm(state, cancel_controller);
    }
    handle_escape(state)
}

/// A wheel notch goes through the same modal gates as a key: an open info
/// panel or picker owns the notch (the inline slash list passes it through), and
/// the permission card swallows it. The gates report a repaint exactly as the
/// lens tuples did.
fn reduce_wheel_event(state: &mut AppState, up: bool, column: u16, row: u16) -> bool {
    if state.info_panel.is_some() {
        return false;
    }
    if state.picker.active().is_some() && state.picker.active() != Some(ActivePicker::InlineSlash) {
        return true;
    }
    if state.permission.has_prompt() {
        return true;
    }
    reduce_wheel(state, up, column, row)
}

/// A wheel notch over the transcript leaves insert mode and scrolls. The
/// insert-to-normal transition mutates the rendered mode indicator and pane
/// focus, so it is a visible change even when the scroll is a no-op.
fn reduce_wheel(state: &mut AppState, up: bool, column: u16, row: u16) -> bool {
    let select = state.input.mode == InputMode::Visual;
    if !state.scroll.contains_transcript_point(column, row) {
        return false;
    }
    let mode_changed = state.input.mode == InputMode::Insert;
    if mode_changed {
        state.enter_normal_mode();
    }
    let action = if up {
        ScrollAction::WheelUp { select }
    } else {
        ScrollAction::WheelDown { select }
    };
    state.scroll.reduce_scroll_action(action) || mode_changed
}

// endregion: --- Key arms

/// A turn-level error is shown as a system notice in the transcript and closes
/// the turn, exactly as the former `TurnError` arm did.
fn handle_turn_error(state: &mut AppState, message: &str) -> bool {
    push_system_notice(state, format!("Error: {message}"));
    state.finalize_cycle();
    state.permission.close_prompt();
    state.turn.finalize(
        &mut state.transcript,
        &mut state.status,
        &mut state.input_locked,
    )
}

/// Push a system line into the transcript. Both the live error path and the
/// replayed snapshot path funnel through here, so a resumed session shows the
/// same notice rows a live one does.
pub(crate) fn push_system_notice(state: &mut AppState, text: String) {
    state
        .transcript
        .push_entry(TranscriptEntry::Notice(NoticeEntry {
            kind: NoticeKind::System,
            text,
        }));
}

fn handle_submit(state: &mut AppState, submitted_text: String) -> bool {
    state.input.pending_submit_text = None;
    if submitted_text.trim().is_empty() {
        return false;
    }

    match parse_slash_command(&submitted_text) {
        SlashParseResult::Command(agnt_core::protocol::slash::SlashCommand::Models) => {
            state.queue_launch_request(SharedUiAction::Models);
            return true;
        }
        SlashParseResult::Command(agnt_core::protocol::slash::SlashCommand::Agent) => {
            state.queue_launch_request(SharedUiAction::Agents);
            return true;
        }
        SlashParseResult::Command(agnt_core::protocol::slash::SlashCommand::Session) => {
            if let Some(session_id) = extract_session_id(&submitted_text) {
                state.queue_switch_request(SwitchRequest::Session(session_id.to_string()));
            } else {
                state.queue_launch_request(SharedUiAction::Sessions);
            }
            return true;
        }
        SlashParseResult::Command(agnt_core::protocol::slash::SlashCommand::Theme) => {
            state.queue_launch_request(SharedUiAction::Themes);
            return true;
        }
        SlashParseResult::Command(_) | SlashParseResult::Unknown(_) => {
            state.enqueue_immediate_submission(submitted_text);
            return true;
        }
        SlashParseResult::NotSlash => {}
    }

    state.enqueue_prompt(submitted_text);
    true
}

fn handle_enter_insert_mode(state: &mut AppState) -> bool {
    if state.phase == UiPhase::Idle {
        state.enter_insert_mode();
        return true;
    }
    false
}

fn handle_enter_visual_mode(state: &mut AppState) -> bool {
    if state.phase != UiPhase::Idle {
        return false;
    }
    // Scroll domain: focus guard, selection start, status lines. The input
    // mode transition stays here (input domain).
    let entered = state.scroll.enter_visual_mode(&mut state.status);
    if entered {
        state.input.mode = InputMode::Visual;
    }
    true
}

fn handle_enter_normal_mode_from_chord(state: &mut AppState) -> bool {
    if state.phase == UiPhase::Idle {
        state.enter_normal_mode();
        return true;
    }
    false
}

fn handle_yank_selection(state: &mut AppState) -> bool {
    if state.input.mode != InputMode::Visual {
        return false;
    }
    // Cross-domain: the payload comes from the transcript model (transcript +
    // scroll + theme domains), the clipboard request stays here (input domain).
    if let Some(payload) = state.yank_selection_payload() {
        state.input.set_clipboard_request(payload);
    }
    state.enter_normal_mode();
    true
}

fn handle_toggle_command_palette(state: &mut AppState) -> bool {
    if state.picker.render_kind() == Some(PickerRenderKind::CommandPalette) {
        state.picker.close();
    } else {
        state.info_panel = None;
        let entry = state.picker.open(ActivePicker::CommandPalette);
        entry.state.options = CommandPaletteAction::PALETTE_ACTIONS
            .iter()
            .map(|a| PickerOption {
                id: a.label().to_string(),
                display: a.label().to_string(),
                search_text: a.label().to_string(),
                section: None,
                sort_key: Vec::new(),
                payload: PickerPayload::Command(*a),
            })
            .collect();
    }
    true
}

fn handle_picker_submit(state: &mut AppState, submit: SubmitAction) -> bool {
    match submit {
        SubmitAction::Switch(req) => {
            state.queue_switch_request(req);
            state.picker.close();
            true
        }
        SubmitAction::Launch(kind) => {
            let action = match kind {
                ActivePicker::Model => SharedUiAction::Models,
                ActivePicker::Agent => SharedUiAction::Agents,
                ActivePicker::Session => SharedUiAction::Sessions,
                ActivePicker::Theme => SharedUiAction::Themes,
                _ => return false,
            };
            state.picker.close();
            state.queue_launch_request(action);
            true
        }
        SubmitAction::Command(action) => {
            if action == crate::state::CommandPaletteAction::Models {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Models);
                return true;
            }
            if action == crate::state::CommandPaletteAction::Agents {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Agents);
                return true;
            }
            if action == crate::state::CommandPaletteAction::Sessions {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Sessions);
                return true;
            }
            if action == crate::state::CommandPaletteAction::Theme {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Themes);
                return true;
            }
            if let Some(panel) = action.info_panel() {
                state.open_info_panel(panel);
            } else {
                state.picker.close();
            }
            true
        }
        SubmitAction::SlashAccept => {
            let Some(selected) = state.picker.selected_slash_label() else {
                return false;
            };
            state.picker.close();
            handle_submit(state, selected)
        }
    }
}

fn handle_quit(state: &mut AppState, cancel_controller: Option<&CancelController>) -> bool {
    if state.phase != crate::state::UiPhase::Idle {
        // Busy: cancel the running turn and quit
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.quit_requested = true;
    } else {
        // Idle: quit
        state.request_quit_if_idle();
    }
    true
}

fn handle_escape(state: &mut AppState) -> bool {
    if state.info_panel.is_some() {
        state.close_info_panel();
        return true;
    }

    if state.phase == UiPhase::Idle && state.input.mode == InputMode::Insert {
        state.enter_normal_mode();
        return true;
    }
    if state.phase == UiPhase::Idle && state.input.mode == InputMode::Visual {
        state.scroll.selection = None;
        state.enter_normal_mode();
        return true;
    }
    if state.request_abort_confirmation() {
        state.status.message.set_message(ESC_ABORT_CONFIRM_STATUS);
        return true;
    }
    false
}

fn handle_escape_confirm(
    state: &mut AppState,
    cancel_controller: Option<&CancelController>,
) -> bool {
    if state.phase == UiPhase::AbortPending && state.abort.pending && state.is_active_cycle() {
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.cancel_and_restore_pending_to_input();
        state.permission.close_prompt();
        state.enter_insert_mode();
        state.status.message.clear();
        return true;
    }
    false
}

/// Attach a permission request's pre-authorize display to the pending call it
/// refers to. The request carries the exact call key, so the pending call is
/// matched byte-for-byte; a decorated display name never equals a key.
pub(crate) fn apply_permission_request_display(
    state: &mut AppState,
    context: &PermissionRequestContext,
) {
    let Some(display) = &context.pre_authorize_display else {
        return;
    };
    let display = crate::state::preview_to_display(display);
    state
        .tool
        .set_preview_for_key(&mut state.transcript, &context.tool_key, &display);
}
