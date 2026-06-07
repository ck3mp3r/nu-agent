use crate::agent::protocol::event::{
    PermissionDecision, PermissionRequestContext, ToolDisplay, ToolDisplaySection, UiEvent,
};
use crate::agent::protocol::slash::{SlashParseResult, parse_slash_command, slash_command_label};
use crate::agent::ui::tui::{
    interaction::cancel::CancelController,
    markdown,
    state::{
        AppState, CompactionStatus, InputMode, PaneFocus, TranscriptRole, UiPhase,
        info_panel_for_command_palette_action,
    },
};

pub const ESC_ABORT_CONFIRM_STATUS: &str = "Hit escape again to abort.";
const ABORT_REQUESTED_STATUS: &str = "Abort requested.";
const VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS: &str =
    "Visual mode requires transcript focus (Tab/h/l).";
const TRANSCRIPT_PAGE_LINES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAction {
    InsertChar(char),
    InsertNewline,
    Submit,
    Backspace,
    Delete,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    HistoryUp,
    HistoryDown,
    ScrollPageUp,
    ScrollPageDown,
    CompleteForward,
    CompleteBackward,
    QueryNext,
    ToggleCommandPalette,
    CommandPaletteMoveUp,
    CommandPaletteMoveDown,
    CommandPaletteSelect,
    CommandPaletteClose,
    InlineSlashMoveUp,
    InlineSlashMoveDown,
    InlineSlashAccept,
    InlineSlashClose,
    Resize { columns: u16, rows: u16 },
    Quit,
    Esc,
    EscConfirm,
    EnterInsertMode,
    EnterNormalModeFromChord,
    ScrollLineUp,
    ScrollLineDown,
    ScrollToTop,
    ScrollToBottom,
    FocusPaneLeft,
    FocusPaneRight,
    EnterVisualMode,
    YankSelection,
    PermissionAllowOnce,
    PermissionAllowAlways,
    PermissionDeny,
    Noop,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ReducerInput {
    User(UserAction),
    Event(UiEvent),
}

pub fn reduce_with_cancel_controller(
    state: &mut AppState,
    input: ReducerInput,
    cancel_controller: Option<&CancelController>,
) {
    match input {
        ReducerInput::User(action) => reduce_user_action(state, action, cancel_controller),
        ReducerInput::Event(event) => reduce_ui_event(state, event),
    }
}

fn reduce_user_action(
    state: &mut AppState,
    action: UserAction,
    cancel_controller: Option<&CancelController>,
) {
    match action {
        UserAction::InsertChar(ch) => handle_insert_char(state, ch),
        UserAction::InsertNewline => handle_insert_newline(state),
        UserAction::Backspace => handle_backspace(state),
        UserAction::Delete => handle_delete(state),
        UserAction::Submit => handle_submit(state),
        UserAction::MoveCursorLeft => handle_move_cursor_left(state),
        UserAction::MoveCursorRight => handle_move_cursor_right(state),
        UserAction::MoveCursorHome => handle_move_cursor_home(state),
        UserAction::MoveCursorEnd => handle_move_cursor_end(state),
        UserAction::Noop => {}
        UserAction::EnterInsertMode => handle_enter_insert_mode(state),
        UserAction::EnterVisualMode => handle_enter_visual_mode(state),
        UserAction::EnterNormalModeFromChord => handle_enter_normal_mode_from_chord(state),
        UserAction::ScrollLineUp => handle_scroll_line_up(state),
        UserAction::ScrollLineDown => handle_scroll_line_down(state),
        UserAction::ScrollToTop => handle_scroll_to_top(state),
        UserAction::ScrollToBottom => handle_scroll_to_bottom(state),
        UserAction::FocusPaneLeft => handle_focus_pane_left(state),
        UserAction::FocusPaneRight => handle_focus_pane_right(state),
        UserAction::YankSelection => handle_yank_selection(state),
        UserAction::PermissionAllowOnce => {
            let _ = state.submit_permission_decision(PermissionDecision::AllowOnce);
        }
        UserAction::PermissionAllowAlways => {
            let _ = state.submit_permission_decision(PermissionDecision::AllowAlways);
        }
        UserAction::PermissionDeny => {
            let _ = state.submit_permission_decision(PermissionDecision::Deny);
        }
        UserAction::Resize { .. } => {}
        UserAction::ToggleCommandPalette => handle_toggle_command_palette(state),
        UserAction::CommandPaletteMoveUp => state.command_palette_move_up(),
        UserAction::CommandPaletteMoveDown => state.command_palette_move_down(),
        UserAction::CommandPaletteSelect => handle_command_palette_select(state),
        UserAction::CommandPaletteClose => state.close_command_palette(),
        UserAction::InlineSlashMoveUp => state.inline_slash_move_up(),
        UserAction::InlineSlashMoveDown => state.inline_slash_move_down(),
        UserAction::InlineSlashAccept => handle_inline_slash_accept(state),
        UserAction::InlineSlashClose => state.close_inline_slash_suggestions(),
        UserAction::HistoryUp
        | UserAction::HistoryDown
        | UserAction::QueryNext
        | UserAction::CompleteForward
        | UserAction::CompleteBackward => {}
        UserAction::ScrollPageUp => handle_scroll_page_up(state),
        UserAction::ScrollPageDown => handle_scroll_page_down(state),
        UserAction::Quit => handle_quit(state),
        UserAction::Esc => handle_escape(state),
        UserAction::EscConfirm => {
            handle_escape_confirm(state, cancel_controller);
        }
    }
}

fn handle_insert_char(state: &mut AppState, ch: char) {
    state.append_input_char(ch);
}

fn handle_insert_newline(state: &mut AppState) {
    state.insert_input_newline();
}

fn handle_backspace(state: &mut AppState) {
    state.backspace_input_char();
}

fn handle_delete(state: &mut AppState) {
    state.delete_input_char();
}

fn handle_submit(state: &mut AppState) {
    let submitted_text = state.input.buffer.clone();
    if submitted_text.trim().is_empty() {
        return;
    }

    match parse_slash_command(&submitted_text) {
        SlashParseResult::Command(crate::agent::protocol::slash::SlashCommand::Models) => {
            state.queue_model_picker_launch_request();
            return;
        }
        SlashParseResult::Command(crate::agent::protocol::slash::SlashCommand::Agent) => {
            state.queue_agent_picker_launch_request();
            return;
        }
        SlashParseResult::Command(_) | SlashParseResult::Unknown(_) => {
            state.enqueue_immediate_submission(submitted_text);
            return;
        }
        SlashParseResult::NotSlash => {}
    }

    state.enqueue_prompt(submitted_text);
    state.input.buffer.clear();
    state.input.cursor = 0;
}

fn handle_move_cursor_left(state: &mut AppState) {
    state.move_cursor_left();
}

fn handle_move_cursor_right(state: &mut AppState) {
    state.move_cursor_right();
}

fn handle_move_cursor_home(state: &mut AppState) {
    state.move_cursor_home();
}

fn handle_move_cursor_end(state: &mut AppState) {
    state.move_cursor_end();
}

fn handle_enter_insert_mode(state: &mut AppState) {
    if state.phase == UiPhase::Idle {
        state.enter_insert_mode();
    }
}

fn handle_enter_visual_mode(state: &mut AppState) {
    // Visual mode is not supported with ListState - make this a no-op
    if state.phase == UiPhase::Idle {
        if state.pane_focus == PaneFocus::Transcript {
            // No-op: visual mode to be retrofitted later
            state.status_line = "Visual mode not available".to_string();
        } else {
            state.status_line = VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS.to_string();
        }
    }
}

fn handle_enter_normal_mode_from_chord(state: &mut AppState) {
    if state.phase == UiPhase::Idle {
        state.backspace_input_char();
        state.enter_normal_mode();
    }
}

fn handle_scroll_line_up(state: &mut AppState) {
    // Visual mode removed - just scroll
    state.scroll_transcript_line_up();
}

fn handle_scroll_line_down(state: &mut AppState) {
    // Visual mode removed - just scroll
    state.scroll_transcript_line_down();
}

fn handle_scroll_to_top(state: &mut AppState) {
    // Visual mode removed - just scroll
    state.scroll_transcript_to_top();
}

fn handle_scroll_to_bottom(state: &mut AppState) {
    // Visual mode removed - just scroll
    state.scroll_transcript_to_bottom();
}

fn handle_focus_pane_left(state: &mut AppState) {
    state.focus_prev_pane();
}

fn handle_focus_pane_right(state: &mut AppState) {
    state.focus_next_pane();
}

fn handle_yank_selection(state: &mut AppState) {
    // Visual mode removed - make this a no-op
    if state.input_mode == InputMode::Visual {
        state.enter_normal_mode();
    }
}

fn handle_toggle_command_palette(state: &mut AppState) {
    if state.command_palette_open {
        state.close_command_palette();
    } else {
        state.open_command_palette();
    }
}

fn handle_command_palette_select(state: &mut AppState) {
    if let Some(action) = state.command_palette_selected_action() {
        if action == crate::agent::ui::tui::state::CommandPaletteAction::Models {
            state.close_command_palette();
            state.queue_model_picker_launch_request();
            return;
        }

        if action == crate::agent::ui::tui::state::CommandPaletteAction::Agents {
            state.close_command_palette();
            state.queue_agent_picker_launch_request();
            return;
        }

        if let Some(panel) = info_panel_for_command_palette_action(action) {
            state.open_info_panel(panel);
        } else {
            state.close_command_palette();
        }
    }
}

fn handle_inline_slash_accept(state: &mut AppState) {
    let Some(command) = state.inline_slash_selected_command() else {
        return;
    };
    let selected = slash_command_label(command).to_string();
    state.input.buffer = selected;
    state.input.cursor = state.input.buffer.len();
    state.ensure_invariants();
    handle_submit(state);
}

fn handle_scroll_page_up(state: &mut AppState) {
    // Visual mode removed - just scroll
    state.scroll_transcript_page_up(TRANSCRIPT_PAGE_LINES);
}

fn handle_scroll_page_down(state: &mut AppState) {
    // Visual mode removed - just scroll
    state.scroll_transcript_page_down(TRANSCRIPT_PAGE_LINES);
}

fn handle_quit(state: &mut AppState) {
    state.request_quit_if_idle();
}

fn handle_escape(state: &mut AppState) {
    if state.has_permission_prompt() {
        let _ = state.submit_permission_decision(PermissionDecision::Deny);
        return;
    }

    if state.info_panel.is_some() {
        state.close_info_panel();
        return;
    }

    if state.phase == UiPhase::Idle && state.input_mode == InputMode::Insert {
        state.enter_normal_mode();
        return;
    }
    if state.phase == UiPhase::Idle && state.input_mode == InputMode::Visual {
        state.enter_normal_mode();
        return;
    }
    if state.request_abort_confirmation() {
        state.status_line = ESC_ABORT_CONFIRM_STATUS.to_string();
    }
}

fn handle_escape_confirm(state: &mut AppState, cancel_controller: Option<&CancelController>) {
    if state.phase == UiPhase::AbortPending && state.abort.pending && state.is_active_cycle() {
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.cancel_active_and_pending_prompts();
        state.status_line = ABORT_REQUESTED_STATUS.to_string();
    }
}

fn reduce_ui_event(state: &mut AppState, event: UiEvent) {
    match event {
        UiEvent::LlmStart => handle_llm_start(state),
        UiEvent::Tick => handle_tick(state),
        UiEvent::ToolStart {
            name, arguments, ..
        } => handle_tool_start(state, &name, &arguments),
        UiEvent::ToolEnd {
            name,
            arguments,
            success,
            display,
            ..
        } => handle_tool_end(state, &name, &arguments, success, display),
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => handle_permission_requested(state, request_id, context),
        UiEvent::PermissionDecisionSubmitted { .. }
        | UiEvent::PermissionDecisionTimedOut { .. }
        | UiEvent::PermissionDecisionIgnored { .. } => {}
        UiEvent::LlmEnd {
            response_chars,
            input_tokens,
            output_tokens,
            total_tokens,
            ..
        } => handle_llm_end(
            state,
            response_chars,
            input_tokens,
            output_tokens,
            total_tokens,
        ),
        UiEvent::Warning { message } => handle_warning(state, message),
        UiEvent::TurnError { message } => {
            state.push_transcript_line(TranscriptRole::System, format!("Error: {}", message));
            state.status_line = message.clone();
            finalize(state);
        }
        UiEvent::CompactionStarted { source } => {
            state.start_compaction_block(&source);
        }
        UiEvent::CompactionSummaryChunk {
            source, aggregated, ..
        } => {
            handle_compaction_summary_chunk(state, &source, aggregated);
        }
        UiEvent::CompactionTriggered {
            source,
            summarized_count: _,
            kept_recent_count: _,
            summary_preview: _,
            summary_body,
        } => {
            state.start_compaction_block(&source);
            state.finish_compaction_block(&source, CompactionStatus::Done);
            let body = if summary_body.trim().is_empty() {
                "(empty summary)".to_string()
            } else {
                summary_body
            };

            // Clear streaming state before final render pass
            if let Some(start) = state.compaction_streaming_start {
                state.transcript_preview.truncate(start);
            }

            for line in state.project_assistant_markdown_lines(&body) {
                let text = markdown::rendered_line_to_plain_text(&line);
                if text.trim().is_empty() {
                    continue;
                }
                state.push_transcript_rendered_line(TranscriptRole::Compaction, line);
            }
            state.compaction_streaming_start = None;
            state.push_transcript_line(TranscriptRole::Separator, String::new());
            state.status_line.clear();
        }
        UiEvent::CompactionFailed { source, message } => {
            state.start_compaction_block(&source);
            state.finish_compaction_block(&source, CompactionStatus::Failed);
            state.push_transcript_line(
                TranscriptRole::System,
                format!("Compaction failed deterministically: {message}"),
            );
            state.status_line.clear();
        }
        UiEvent::AssistantMessage { text } => {
            log::trace!("reducer: AssistantMessage text_len={}", text.len());
            handle_assistant_message(state, text);
        }
        UiEvent::Completed { .. } => finalize(state),
    }
}

fn handle_llm_start(state: &mut AppState) {
    if state.phase == UiPhase::Idle {
        state.phase = UiPhase::Busy;
        state.ensure_invariants();
    }
    // Reset streaming state at the start of a new LLM response
    state.streaming_message_start = None;
}

fn handle_tick(state: &mut AppState) {
    if state.status_line.is_empty() {
        state.status_line = "Thinking...".to_string();
    }
}

fn handle_tool_start(state: &mut AppState, name: &str, arguments: &str) {
    state.start_tool_call(name, arguments);
    state.status_line = format!("Tool: {name}");
}

fn handle_permission_requested(
    state: &mut AppState,
    request_id: String,
    context: PermissionRequestContext,
) {
    if let Some(display) = &context.pre_authorize_display {
        append_direct_tool_display(state, display.clone());

        if let Some(tool_key) = state.latest_in_progress_tool_key_for_tool(&context.tool) {
            state.pre_displayed_tool_keys.insert(tool_key);
        }
    }

    state.open_permission_prompt(crate::agent::ui::tui::state::PermissionPrompt {
        request_id,
        matched_rule_identity: context.matched_rule_identity,
        tool: context.tool,
        source: context.source,
        mode: context.mode,
        scope: context.scope,
        pattern: context.pattern,
        target_field: context.target_field,
        summary: context.summary,
    });
}

fn append_direct_tool_display(state: &mut AppState, display: ToolDisplay) {
    let suppress_title = should_suppress_redundant_edit_title(&display);
    let suppress_single_section_stats = suppress_title && display.sections.len() == 1;

    if !suppress_title {
        state.push_transcript_line(TranscriptRole::ToolDisplay, display.title);
    }

    for section in display.sections {
        append_direct_tool_display_section(state, section, suppress_single_section_stats);
    }
}

fn should_suppress_redundant_edit_title(display: &ToolDisplay) -> bool {
    display.title.starts_with("edit ")
        && display.sections.len() == 1
        && display.sections[0].language == "diff"
}

fn append_direct_tool_display_section(
    state: &mut AppState,
    section: ToolDisplaySection,
    suppress_stats_line: bool,
) {
    state.push_transcript_line(
        TranscriptRole::ToolDisplay,
        format!("{} ({})", section.label, section.language),
    );

    if !suppress_stats_line && let Some(stats) = section.stats {
        let mut stat_parts = Vec::new();
        if let Some(files_changed) = stats.files_changed {
            stat_parts.push(format!("files={files_changed}"));
        }
        if let Some(insertions) = stats.insertions {
            stat_parts.push(format!("+{insertions}"));
        }
        if let Some(deletions) = stats.deletions {
            stat_parts.push(format!("-{deletions}"));
        }
        if let Some(true) = stats.diff_truncated {
            stat_parts.push("truncated=true".to_string());
        }
        if !stat_parts.is_empty() {
            state.push_transcript_line(TranscriptRole::ToolDisplay, stat_parts.join(" "));
        }
    }

    let section_content = if section.language == "diff" {
        add_diff_line_number_readability(&section.content)
    } else {
        section.content
    };

    let markdown = format!("```{}\n{}\n```", section.language, section_content);
    for rendered_line in state.project_assistant_markdown_lines(&markdown) {
        let text = markdown::rendered_line_to_plain_text(&rendered_line);
        if text.trim().is_empty() {
            continue;
        }
        state.push_transcript_rendered_line(TranscriptRole::ToolDisplay, rendered_line);
    }
}

fn handle_tool_end(
    state: &mut AppState,
    name: &str,
    arguments: &str,
    success: bool,
    display: Option<ToolDisplay>,
) {
    state.finish_tool_call(name, arguments, success);

    let tool_key = format!("{name}\n{arguments}");
    if state.pre_displayed_tool_keys.remove(&tool_key) {
        // Display was already pushed during permission request - skip
    } else if let Some(display) = display {
        append_direct_tool_display(state, display);
    }

    state.status_line = "Thinking...".to_string();
}

fn parse_hunk_start(line: &str, prefix: char) -> Option<usize> {
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == prefix {
            let remainder = chars.as_str();
            let digits: String = remainder
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if digits.is_empty() {
                return None;
            }
            return digits.parse::<usize>().ok();
        }
    }
    None
}

fn add_diff_line_number_readability(diff: &str) -> String {
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;
    let mut out = String::new();

    for segment in diff.split_inclusive('\n') {
        let (line, newline) = if let Some(stripped) = segment.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (segment, "")
        };

        if line.starts_with("@@") {
            old_line = parse_hunk_start(line, '-');
            new_line = parse_hunk_start(line, '+');
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("\\ ") {
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        let mut chars = line.chars();
        let prefix = chars.next();
        let body = chars.as_str();

        match (prefix, old_line, new_line) {
            (Some(' '), Some(old), Some(new)) => {
                out.push_str(&format!(" {:>4} {:>4} │{}{}", old, new, body, newline));
                old_line = Some(old.saturating_add(1));
                new_line = Some(new.saturating_add(1));
            }
            (Some('-'), Some(old), _) => {
                out.push_str(&format!("-{:>4}      │{}{}", old, body, newline));
                old_line = Some(old.saturating_add(1));
            }
            (Some('+'), _, Some(new)) => {
                out.push_str(&format!("+     {:>4} │{}{}", new, body, newline));
                new_line = Some(new.saturating_add(1));
            }
            _ => {
                out.push_str(line);
                out.push_str(newline);
            }
        }
    }

    out
}

fn handle_llm_end(
    state: &mut AppState,
    response_chars: usize,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
) {
    state.record_token_usage(input_tokens, output_tokens, total_tokens);
    state.status_line = format!("Response ready ({response_chars} chars)");
}

fn handle_warning(state: &mut AppState, message: String) {
    state.status_line = message;
}

fn handle_assistant_message(state: &mut AppState, text: String) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    // If this is the first delta, record where the message starts in transcript
    if state.streaming_message_start.is_none() {
        state.streaming_message_start = Some(state.transcript_preview.len());
    }

    // Remove previous rendering of this message
    if let Some(start) = state.streaming_message_start {
        state.transcript_preview.truncate(start);
        state.clear_assistant_projection_cache();
    }

    // Project the full accumulated text through markdown
    let projected_lines = state.project_assistant_markdown_lines(trimmed);
    if assistant_diff_regurgitation_is_redundant(state, &projected_lines) {
        return;
    }

    // Always follow tail with ListState
    state.scroll_transcript_to_bottom();
    for line in projected_lines {
        let text = markdown::rendered_line_to_plain_text(&line);
        if text.trim().is_empty() {
            continue;
        }
        state.push_transcript_rendered_line(TranscriptRole::Assistant, line);
    }
}

fn handle_compaction_summary_chunk(state: &mut AppState, source: &str, text: String) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    // Ensure compaction block is started (idempotent)
    state.start_compaction_block(source);

    // Track streaming start position
    if state.compaction_streaming_start.is_none() {
        state.compaction_streaming_start = Some(state.transcript_preview.len());
    }

    // Remove previous rendering of this streaming message
    if let Some(start) = state.compaction_streaming_start {
        state.transcript_preview.truncate(start);
        state.clear_assistant_projection_cache();
    }

    // Re-project the full accumulated text through markdown
    let projected_lines = state.project_assistant_markdown_lines(trimmed);
    state.scroll_transcript_to_bottom();
    for line in projected_lines {
        let text = markdown::rendered_line_to_plain_text(&line);
        if text.trim().is_empty() {
            continue;
        }
        state.push_transcript_rendered_line(TranscriptRole::Compaction, line);
    }
}

fn assistant_diff_regurgitation_is_redundant(
    state: &AppState,
    assistant_lines: &[ratatui::text::Line<'static>],
) -> bool {
    let latest_tool_display_diff = latest_tool_display_diff_lines(state);
    let Some(latest_tool_display_diff) = latest_tool_display_diff else {
        return false;
    };

    let candidate = assistant_lines
        .iter()
        .map(markdown::rendered_line_to_plain_text)
        .map(|line| normalize_diff_line_for_comparison(line.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if candidate.is_empty() {
        return false;
    }

    let contains_diff_signature = candidate.iter().any(|line| {
        line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ")
    });
    if !contains_diff_signature {
        return false;
    }

    let diff_lines = latest_tool_display_diff
        .iter()
        .map(|line| normalize_diff_line_for_comparison(line.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    candidate.iter().all(|line| {
        diff_lines.contains(line)
            || line.eq_ignore_ascii_case("dry-run diff")
            || line.eq_ignore_ascii_case("dry run diff")
            || line.ends_with(':')
    })
}

fn normalize_diff_line_for_comparison(line: &str) -> String {
    if let Some((_, rhs)) = line.split_once('│') {
        let rhs = rhs.trim_start();
        if line.starts_with('+') {
            return format!("+{rhs}");
        }
        if line.starts_with('-') {
            return format!("-{rhs}");
        }
        return format!(" {rhs}");
    }

    line.to_string()
}

fn latest_tool_display_diff_lines(state: &AppState) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    for entry in state.transcript_preview.iter().rev() {
        if entry.role() == crate::agent::ui::transcript::ir::Role::ToolDisplay {
            lines.push(entry.text());
            continue;
        }

        if !lines.is_empty() {
            break;
        }
    }

    if lines.is_empty() {
        return None;
    }

    lines.reverse();
    Some(
        lines
            .into_iter()
            .filter(|line| {
                line.starts_with("--- ")
                    || line.starts_with("+++ ")
                    || line.starts_with("@@ ")
                    || line.starts_with(' ')
                    || line.starts_with('-')
                    || line.starts_with('+')
                    || line.starts_with('\\')
            })
            .collect(),
    )
}

fn finalize(state: &mut AppState) {
    state.finalize_cycle();
    state.status_line.clear();
    // Reset streaming state when the LLM response is complete
    state.streaming_message_start = None;
}
