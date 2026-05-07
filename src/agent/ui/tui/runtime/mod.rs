use std::{
    io::Write,
    time::{Duration, Instant},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    style::Style,
    layout::Position,
    layout::{Constraint, Direction, Layout},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
mod transcript_window;
mod transcript_rows;
mod status;
mod tool_hydration;
mod terminal_events;
mod terminal_io;
mod render_frame;

#[cfg(test)]
mod test;

#[cfg(test)]
mod hybrid_events_test;

use crate::agent::ui::{renderer::UiRenderer,
    tui::{
        interaction::{
            cancel::CancelController,
            dispatch::dispatch_terminal_event,
            input::{TerminalEvent, TerminalKey},
            reducer::{ReducerInput, reduce_with_cancel_controller},
        },
        platform::{
            safety::{RestoreRunError, run_with_restore},
            terminal::{TerminalBackend, TerminalLifecycle, TerminalLifecycleError},
            transport::TuiTransport,
        },
        rendering::{
            layout::{
                LayoutInput, LayoutOutput, input_cursor_row_col, input_pane_height_for_content,
                recompute_layout, wrapped_input_rows,
            },
            theme::TuiTheme,
        },
        state::{AppState, TranscriptRole},
    },
};
use crate::agent::protocol::event::UiEvent;
#[cfg(test)]
use crate::agent::ui::tui::state::{PromptStatus, TranscriptLineStatus};
use crate::agent::protocol::contracts::UiMessageSnapshot;

use transcript_rows::render_transcript_lines;
use status::{
    availability_label, build_status_lines, compact_status_line, cursor_style_for_mode,
    transcript_selection_range_for_render, transcript_title_for_render,
};
use render_frame::{
    current_time_millis, transcript_height_for_main, vertical_heights_for_main_with_input,
};
#[cfg(test)]
use render_frame::main_pane_rects_for_height;
use tool_hydration::{extract_tool_name, parse_persisted_tool_status_line};
#[cfg(test)]
use status::visual_indicator_line;
#[cfg(test)]
use transcript_rows::{
    build_row_spans, indicator_style_for_status, lane_prefix_spans, prompt_indicator_for_status,
};
use transcript_window::{should_insert_transition_spacer, visible_transcript_window_for_render};
#[allow(unused_imports)]
pub use terminal_events::{
    CrosstermTerminalEvents, HybridTerminalEvents, InputSourceDiagnostics, TerminalEventSource,
};
#[cfg(test)]
pub use terminal_events::ScriptedTerminalEvents;
pub use terminal_io::{TtyTerminalEvents, open_tty_reader};
#[cfg(test)]
pub(crate) use terminal_events::map_crossterm_event_for_test;

#[derive(Debug)]
pub struct RuntimeCoordinator {
    state: AppState,
    transport: TuiTransport,
    cancel_controller: CancelController,
    layout: LayoutOutput,
    side_pane_visible: Option<bool>,
    quit_requested: bool,
    fatal_error: Option<String>,
    active_model_identity: String,
    input_backend_status: String,
    last_input_poll_status: String,
    last_input_error: Option<String>,
    input_watchdog_started_at: Instant,
    input_watchdog_timeout: Duration,
    theme: TuiTheme,
}

impl RuntimeCoordinator {
    const DEFAULT_INPUT_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(5);

    pub fn new(columns: u16, rows: u16, side_pane_visible: Option<bool>) -> Self {
        Self::new_with_watchdog(
            columns,
            rows,
            side_pane_visible,
            Self::DEFAULT_INPUT_WATCHDOG_TIMEOUT,
        )
    }

    pub(crate) fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
        for message in messages {
            let role = match message.role() {
                "user" => TranscriptRole::User,
                "assistant" => TranscriptRole::Assistant,
                "tool" => TranscriptRole::Tool,
                _ => TranscriptRole::System,
            };
            let message_content = message.content();
            if message_content.trim().is_empty() {
                continue;
            }
            if role == TranscriptRole::Assistant {
                for line in self.state.project_assistant_markdown_lines(message_content) {
                    let plain_text = crate::agent::ui::tui::markdown::rendered_line_to_plain_text(&line);
                    if !plain_text.trim().is_empty() {
                        self.state.push_transcript_rendered_line(role, line);
                    }
                }
                continue;
            }

            if role == TranscriptRole::Tool {
                let persisted = message_content.trim();
                if let Some(arguments) = message.tool_arguments() {
                    let success = message.tool_success().unwrap_or(true);
                    self.state.start_tool_call(extract_tool_name(persisted), arguments);
                    self.state
                        .finish_tool_call(extract_tool_name(persisted), arguments, success);
                    continue;
                }
                if let Some((name, arguments, success)) = parse_persisted_tool_status_line(persisted) {
                    self.state.start_tool_call(name, arguments);
                    self.state.finish_tool_call(name, arguments, success);
                    continue;
                }
            }

            for line in message_content.lines() {
                if !line.trim().is_empty() {
                    self.state.push_transcript_line(role, line.to_string());
                }
            }
        }
    }

    fn new_with_watchdog(
        columns: u16,
        rows: u16,
        _side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        let side_pane_visible = Some(false);
        let layout = recompute_layout(LayoutInput {
            columns,
            rows,
            side_pane_visible,
            input_height: None,
        });
        let mut coordinator = Self {
            state: AppState::new(),
            transport: TuiTransport::new(),
            cancel_controller: CancelController::new(),
            layout,
            side_pane_visible,
            quit_requested: false,
            fatal_error: None,
            active_model_identity: "unknown".to_string(),
            input_backend_status: "unknown".to_string(),
            last_input_poll_status: "waiting for input poll".to_string(),
            last_input_error: None,
            input_watchdog_started_at: Instant::now(),
            input_watchdog_timeout,
            theme: TuiTheme::default(),
        };
        coordinator.sync_transcript_viewport_lines_with_layout();
        coordinator
    }

    #[cfg(test)]
    pub fn new_for_test_with_watchdog(
        columns: u16,
        rows: u16,
        side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        Self::new_with_watchdog(columns, rows, side_pane_visible, input_watchdog_timeout)
    }

    #[cfg(test)]
    pub fn state(&self) -> &AppState {
        &self.state
    }

    #[cfg(test)]
    pub fn layout(&self) -> LayoutOutput {
        self.layout
    }

    #[cfg(test)]
    pub fn cancel_controller(&self) -> &CancelController {
        &self.cancel_controller
    }

    #[cfg(test)]
    pub fn input_diagnostics_snapshot(&self) -> (String, String, Option<String>) {
        (
            self.input_backend_status.clone(),
            self.last_input_poll_status.clone(),
            self.last_input_error.clone(),
        )
    }

    pub(crate) fn take_submitted_prompt(&mut self) -> Option<String> {
        self.state.take_next_prompt_for_execution()
    }

    pub(crate) fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.active_model_identity = active_model_identity;
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.cancel_controller.take_cancel_requested()
    }

    pub fn enqueue_ui_event(&mut self, event: UiEvent) {
        self.transport.enqueue_ui_event(event);
    }

    pub fn poll_terminal_event(&mut self, event_source: &mut impl TerminalEventSource) {
        let poll_result = event_source.poll_event();
        let diagnostics = event_source.diagnostics();
        self.update_input_diagnostics(&diagnostics);

        let event = match poll_result {
            Ok(Some(event)) => event,
            Ok(None) => {
                self.maybe_trigger_input_watchdog(&diagnostics);
                return;
            }
            Err(error) => {
                if Self::both_backends_unavailable(&diagnostics) {
                    self.trigger_no_interactive_backend_fail_fast(Some(error));
                    return;
                }
                self.state.status_line = format!("Terminal input error: {error}");
                self.fatal_error = Some(self.state.status_line.clone());
                self.quit_requested = true;
                self.cancel_controller.request_cancel();
                return;
            }
        };

        self.last_input_poll_status = format!(
            "event from {}",
            diagnostics.active_backend
        );

        if let TerminalEvent::Key(TerminalKey::Esc) = event
            && self.state.phase == crate::agent::ui::tui::state::UiPhase::Idle
        {
            self.state.status_line = "Esc pressed. Press Ctrl+C to quit.".to_string();
        }

        if let TerminalEvent::Key(TerminalKey::CtrlC) = event {
            self.quit_requested = true;
            self.cancel_controller.request_cancel();
        }

        if let TerminalEvent::Resize(resize) = event {
            let input_height = input_pane_height_for_content(
                &self.state.input.buffer,
                resize.columns,
            );
            self.layout = recompute_layout(LayoutInput {
                columns: resize.columns,
                rows: resize.rows,
                side_pane_visible: self.side_pane_visible,
                input_height: Some(input_height),
            });
        }

        let _ = dispatch_terminal_event(&mut self.state, &event, Some(&self.cancel_controller));
        self.recompute_layout_for_current_input();
        self.flush_clipboard_request();
        self.quit_requested |= self.state.quit_requested;

        self.sync_transcript_viewport_lines_with_layout();
    }

    fn sync_transcript_viewport_lines_with_layout(&mut self) {
        let main_height = self
            .layout
            .transcript
            .height
            .saturating_add(self.layout.status_event.height)
            .saturating_add(self.layout.input.height);
        let transcript_height = Self::transcript_height_for_main(main_height);
        let visible_lines = transcript_height.saturating_sub(2) as usize;
        self.state.set_transcript_viewport_lines(visible_lines.max(1));
    }

    fn recompute_layout_for_current_input(&mut self) {
        let input_height = input_pane_height_for_content(
            &self.state.input.buffer,
            self.layout.transcript.width,
        );
        let total_rows = self
            .layout
            .transcript
            .height
            .saturating_add(self.layout.status_event.height)
            .saturating_add(self.layout.input.height);
        let total_columns = self
            .layout
            .transcript
            .width
            .saturating_add(self.layout.side_pane.map(|s| s.width).unwrap_or(0));
        self.layout = recompute_layout(LayoutInput {
            columns: total_columns,
            rows: total_rows,
            side_pane_visible: self.side_pane_visible,
            input_height: Some(input_height),
        });
    }

    fn flush_clipboard_request(&mut self) {
        let Some(text) = self.state.take_clipboard_request() else {
            return;
        };

        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(()) => {
                self.state.status_line = "Copied selection to clipboard.".to_string();
            }
            Err(error) => {
                self.state.status_line = format!("Clipboard copy failed: {error}");
            }
        }
    }

    fn update_input_diagnostics(&mut self, diagnostics: &InputSourceDiagnostics) {
        let primary = availability_label(diagnostics.primary_available);
        let fallback = availability_label(diagnostics.fallback_available);
        self.input_backend_status = format!(
            "active={}, crossterm={}, /dev/tty={}",
            diagnostics.active_backend, primary, fallback
        );
        self.last_input_poll_status = diagnostics.last_poll_state.clone();
        self.last_input_error = diagnostics.last_error.clone();
    }

    fn maybe_trigger_input_watchdog(&mut self, diagnostics: &InputSourceDiagnostics) {
        if self.quit_requested || self.fatal_error.is_some() {
            return;
        }

        if !Self::both_backends_unavailable(diagnostics) {
            return;
        }

        if self.input_watchdog_started_at.elapsed() < self.input_watchdog_timeout {
            return;
        }

        self.trigger_no_interactive_backend_fail_fast(None);
    }

    fn both_backends_unavailable(diagnostics: &InputSourceDiagnostics) -> bool {
        diagnostics.primary_available == Some(false) && diagnostics.fallback_available == Some(false)
    }

    fn trigger_no_interactive_backend_fail_fast(&mut self, poll_error: Option<String>) {
        if let Some(error) = poll_error
            && self.last_input_error.is_none()
        {
            self.last_input_error = Some(error);
        }

        let mut message = format!(
            "No interactive input backend available (crossterm and /dev/tty unavailable). Last poll: {}.",
            self.last_input_poll_status
        );
        if let Some(error) = self.last_input_error.as_deref() {
            message.push_str(&format!(" Last error: {error}."));
        }
        message.push_str(" Run `agent` in an interactive terminal and verify TTY access.");

        self.state.status_line = message.clone();
        self.fatal_error = Some(message);
        self.quit_requested = true;
        self.cancel_controller.request_cancel();
    }

    pub fn drain_transport(&mut self) {
        while let Some(item) = self.transport.poll_next() {
            reduce_with_cancel_controller(
                &mut self.state,
                ReducerInput::from(item),
                Some(&self.cancel_controller),
            );
        }
    }

    fn render_frame(&self, live: &mut Option<LiveTerminalUi>) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };

        live.terminal
            .draw(|frame| {
                let area = frame.area();
                let has_side = self.layout.side_pane.is_some();
                let horizontal = if has_side {
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                        .split(area)
                } else {
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(100)])
                        .split(area)
                };

                let main = horizontal[0];
                let (header_h, transcript_h, status_h, input_h) =
                    Self::vertical_heights_for_main_with_input(main.height, self.layout.input.height);
                let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(header_h),
                        Constraint::Length(transcript_h),
                        Constraint::Length(input_h),
                        Constraint::Length(status_h),
                    ])
                    .split(main);

                let header = Paragraph::new(Line::from(vec![Span::raw(
                    "nu-agent TUI  |  Ctrl+C quit  |  Esc/Esc abort  |  PgUp/PgDn/Ctrl+U/Ctrl+D scroll  |  Normal: h/l or Tab pane, j/k, gg/G, v visual",
                )]));
                frame.render_widget(header, vertical[0]);

                let (window_start, window_lines) = visible_transcript_window_for_render(
                    &self.state.transcript_preview,
                    vertical[1].height.saturating_sub(1) as usize,
                    self.state.transcript_scroll_lines_from_bottom,
                    self.state.transcript_follow_tail,
                    vertical[1].width as usize,
                );
                let selected = transcript_selection_range_for_render(
                    &self.state,
                    self.state.transcript_preview.len(),
                );
                let mut transcript = Vec::new();
                let mut prev_role: Option<TranscriptRole> = None;
                for (offset, line) in window_lines.into_iter().enumerate() {
                    let global_idx = window_start.saturating_add(offset);
                    if should_insert_transition_spacer(prev_role, line.role) {
                        transcript.push(Line::from(vec![Span::raw(" ")]));
                    }
                    let line_status = self.state.transcript_line_status_for_index(global_idx);
                    let is_cursor_line = self.state.transcript_cursor_index() == Some(global_idx)
                        && self.state.input_mode
                            != crate::agent::ui::tui::state::InputMode::Insert;
                    let is_selected = selected
                        .map(|(start, end)| global_idx >= start && global_idx <= end)
                        .unwrap_or(false);
                    transcript.extend(render_transcript_lines(
                        line,
                        vertical[1].width as usize,
                        is_selected,
                        is_cursor_line,
                        line_status,
                        current_time_millis(),
                        &self.theme,
                    ));
                    prev_role = self
                        .state
                        .transcript_preview
                        .get(global_idx)
                        .map(|entry| entry.role);
                }
                let transcript_view_height = vertical[1].height.saturating_sub(1) as usize;
                let _transcript_title = transcript_title_for_render(
                    &self.state,
                    self.state.transcript_preview.len(),
                );
                let transcript_border_style = if self.state.pane_focus
                    == crate::agent::ui::tui::state::PaneFocus::Transcript
                {
                    self.theme.focus
                } else {
                    Style::default()
                };
                let transcript_widget = if transcript_view_height == 0 {
                    Paragraph::new(Text::from(Vec::<Line>::new()))
                        .block(Block::default().borders(Borders::TOP).border_style(transcript_border_style))
                        .wrap(Wrap { trim: false })
                } else {
                    Paragraph::new(Text::from(transcript))
                        .block(Block::default().borders(Borders::TOP).border_style(transcript_border_style))
                        .wrap(Wrap { trim: false })
                };
                if vertical[1].height > 0 {
                    frame.render_widget(Clear, vertical[1]);
                    frame.render_widget(transcript_widget, vertical[1]);
                }

                let compact_status = compact_status_line(
                    &self.state,
                    &self.active_model_identity,
                    &self.input_backend_status,
                    &self.last_input_poll_status,
                    self.last_input_error.as_deref(),
                );
                let status_widget = Paragraph::new(Line::from(vec![
                    Span::styled("ℹ ", self.theme.subtle_meta),
                    Span::raw(compact_status),
                ]))
                    .block(Block::default())
                    .wrap(Wrap { trim: false });
                if vertical[3].height > 0 {
                    frame.render_widget(Clear, vertical[3]);
                    frame.render_widget(status_widget, vertical[3]);
                }

                let input_rows = wrapped_input_rows(
                    &self.state.input.buffer,
                    vertical[2].width.saturating_sub(2) as usize,
                );
                let input_border_style = if self.state.pane_focus
                    == crate::agent::ui::tui::state::PaneFocus::Input
                {
                    self.theme.focus
                } else {
                    Style::default()
                };
                let mut input_lines = Vec::new();
                if let Some((first, rest)) = input_rows.split_first() {
                    input_lines.push(Line::from(vec![
                        Span::styled("❯ ", self.theme.input_prompt),
                        Span::raw(first.clone()),
                    ]));
                    for row in rest {
                        input_lines.push(Line::from(vec![Span::raw("  "), Span::raw(row.clone())]));
                    }
                }
                let input_widget = Paragraph::new(Text::from(input_lines))
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(input_border_style),
                )
                .wrap(Wrap { trim: false });
                if vertical[2].height > 0 {
                    frame.render_widget(Clear, vertical[2]);
                    frame.render_widget(input_widget, vertical[2]);
                }

                if !self.state.input.locked && vertical[2].height >= 2 && vertical[2].width >= 1 {
                    let (cursor_row, cursor_col) = input_cursor_row_col(
                        &self.state.input.buffer,
                        self.state.input.cursor,
                        vertical[2].width.saturating_sub(2) as usize,
                    );
                    let x = vertical[2]
                        .x
                        .saturating_add(2)
                        .saturating_add(cursor_col);
                    let max_x = vertical[2]
                        .x
                        .saturating_add(vertical[2].width.saturating_sub(1));
                    let y = vertical[2]
                        .y
                        .saturating_add(1)
                        .saturating_add(cursor_row)
                        .min(vertical[2].y.saturating_add(vertical[2].height.saturating_sub(1)));
                    frame.set_cursor_position(Position {
                        x: x.min(max_x),
                        y,
                    });
                }

                if has_side {
                    let side = horizontal[1];
                    let side_widget = Paragraph::new(Line::from("Events pane reserved"))
                        .block(Block::default().borders(Borders::ALL).title("Events"));
                    frame.render_widget(side_widget, side);
                }
            })
            .map_err(|err| format!("TUI render failed: {err}"))?;

        let cursor_style = cursor_style_for_mode(self.state.input_mode);
        let _ = crossterm::execute!(std::io::stdout(), cursor_style);

        Ok(())
    }

    fn vertical_heights_for_main_with_input(main_height: u16, input_target_height: u16) -> (u16, u16, u16, u16) {
        vertical_heights_for_main_with_input(main_height, input_target_height)
    }

    fn transcript_height_for_main(main_height: u16) -> u16 {
        transcript_height_for_main(main_height)
    }

    #[cfg(test)]
    pub(super) fn main_pane_rects_for_height(
        main_height: u16,
    ) -> (
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
    ) {
        main_pane_rects_for_height(main_height)
    }

    #[cfg(test)]
    pub fn pump_once(&mut self, event_source: &mut impl TerminalEventSource) {
        self.poll_terminal_event(event_source);
        self.drain_transport();
    }
}

#[allow(dead_code)]
fn _legacy_status_lines_for_reference(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> Vec<String> {
    build_status_lines(
        state,
        active_model_identity,
        input_backend_status,
        last_input_poll_status,
        last_input_error,
    )
}

#[cfg(test)]
pub(super) fn compact_status_line_for_test(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> String {
    compact_status_line(
        state,
        active_model_identity,
        input_backend_status,
        last_input_poll_status,
        last_input_error,
    )
}

#[cfg(test)]
pub(super) fn status_lines_for_test(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> Vec<String> {
    build_status_lines(
        state,
        active_model_identity,
        input_backend_status,
        last_input_poll_status,
        last_input_error,
    )
}

#[cfg(test)]
pub(super) fn visual_indicator_line_for_test(state: &AppState) -> Option<String> {
    visual_indicator_line(state)
}

#[cfg(test)]
pub(super) fn transcript_title_for_test(state: &AppState) -> String {
    transcript_title_for_render(state, state.transcript_preview.len())
}

#[cfg(test)]
pub(super) fn cursor_style_for_test(
    mode: crate::agent::ui::tui::state::InputMode,
) -> crossterm::cursor::SetCursorStyle {
    cursor_style_for_mode(mode)
}

#[cfg(test)]
pub(super) fn parse_persisted_tool_status_line_for_test(line: &str) -> Option<(&str, &str, bool)> {
    parse_persisted_tool_status_line(line)
}

#[cfg(test)]
pub(super) use transcript_window::visible_transcript_window;

#[cfg(test)]
pub(super) fn visible_transcript_window_for_render_for_test(
    transcript: &[crate::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
) -> (usize, Vec<crate::agent::ui::tui::state::TranscriptLine>) {
    transcript_window::visible_transcript_window_for_render(
        transcript,
        visible_lines,
        scroll_from_bottom,
        follow_tail,
        content_width,
    )
}

const IN_PROGRESS_SPINNER_FRAMES: [&str; 10] = render_frame::IN_PROGRESS_SPINNER_FRAMES;

#[cfg(test)]
pub(super) fn indicator_style_for_status_for_test(status: TranscriptLineStatus) -> Style {
    indicator_style_for_status(status, &TuiTheme::default())
}

#[cfg(test)]
pub(super) fn transition_spacer_for_roles_for_test(
    previous: Option<TranscriptRole>,
    next: TranscriptRole,
) -> bool {
    should_insert_transition_spacer(previous, next)
}

#[cfg(test)]
pub(super) fn prompt_indicator_for_status_for_test(
    status: PromptStatus,
    now_millis: u128,
) -> &'static str {
    prompt_indicator_for_status(status, now_millis)
}

#[cfg(test)]
pub(super) fn render_transcript_lines_for_test(
    line: crate::agent::ui::tui::state::TranscriptLine,
    line_status: Option<TranscriptLineStatus>,
    now_millis: u128,
) -> Vec<Line<'static>> {
    render_transcript_lines(
        line,
        80,
        false,
        false,
        line_status,
        now_millis,
        &TuiTheme::default(),
    )
}

#[cfg(test)]
pub(super) fn render_transcript_lines_with_flags_for_test(
    line: crate::agent::ui::tui::state::TranscriptLine,
    line_status: Option<TranscriptLineStatus>,
    selected: bool,
    cursor_line: bool,
    width: usize,
    now_millis: u128,
) -> Vec<Line<'static>> {
    render_transcript_lines(
        line,
        width,
        selected,
        cursor_line,
        line_status,
        now_millis,
        &TuiTheme::default(),
    )
}

#[cfg(test)]
pub(super) fn lane_prefix_spans_for_test(
    role: TranscriptRole,
    cursor_line: bool,
) -> Vec<Span<'static>> {
    lane_prefix_spans(role, cursor_line, &TuiTheme::default())
}

#[cfg(test)]
pub(super) fn row_spans_for_test(
    line: crate::agent::ui::tui::state::TranscriptLine,
    line_status: Option<TranscriptLineStatus>,
    cursor_line: bool,
    selected: bool,
    now_millis: u128,
) -> Vec<Span<'static>> {
    build_row_spans(
        &line,
        line_status,
        cursor_line,
        selected,
        now_millis,
        &TuiTheme::default(),
        true,
    )
}

#[cfg(test)]
pub(super) fn input_line_for_test(state: &AppState) -> String {
    let _ = current_time_millis();
    state.input.buffer.clone()
}

#[cfg(test)]
pub(super) fn input_line_for_test_at_millis(state: &AppState, now_millis: u128) -> String {
    let _ = now_millis;
    state.input.buffer.clone()
}

#[cfg(test)]
pub(super) fn input_rows_with_prompt_for_test(state: &AppState, pane_width: u16) -> Vec<String> {
    let rows = wrapped_input_rows(
        &state.input.buffer,
        pane_width.saturating_sub(2) as usize,
    );

    let mut lines = Vec::new();
    if let Some((first, rest)) = rows.split_first() {
        lines.push(format!("❯ {first}"));
        for row in rest {
            lines.push(format!("  {row}"));
        }
    }

    lines
}

pub struct TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    inner: R,
    coordinator: RuntimeCoordinator,
    event_source: E,
    live_terminal: Option<LiveTerminalUi>,
    tui_active: bool,
}

impl<R, E> TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    fn with_terminal_mode(
        inner: R,
        event_source: E,
        columns: u16,
        rows: u16,
        live_terminal: Option<LiveTerminalUi>,
        tui_active: bool,
    ) -> Self {
        Self {
            inner,
            coordinator: RuntimeCoordinator::new(columns, rows, Some(true)),
            event_source,
            live_terminal,
            tui_active,
        }
    }

    fn mark_render_failure(&mut self, error: String) {
        self.coordinator.state.status_line = error.clone();
        self.coordinator.fatal_error = Some(error);
        self.coordinator.quit_requested = true;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(inner: R, event_source: E, columns: u16, rows: u16) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, false)
    }

    pub fn new_live(
        inner: R,
        event_source: E,
        columns: u16,
        rows: u16,
    ) -> Result<Self, String> {
        let live_terminal = LiveTerminalUi::new()?;
        Ok(Self::with_terminal_mode(
            inner,
            event_source,
            columns,
            rows,
            Some(live_terminal),
            true,
        ))
    }

    #[cfg(test)]
    pub fn new_tui_active_for_test(inner: R, event_source: E, columns: u16, rows: u16) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, true)
    }

    #[cfg(test)]
    pub fn coordinator(&self) -> &RuntimeCoordinator {
        &self.coordinator
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.coordinator.take_cancel_requested()
    }

    pub(crate) fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.coordinator
            .set_active_model_identity(active_model_identity);
    }

    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.coordinator.fatal_error()
    }

    pub(crate) fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
        self.coordinator.hydrate_transcript_from_messages(messages);
    }

    pub fn pump_terminal_once(&mut self) {
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.drain_transport();
        if let Err(error) = self.coordinator.render_frame(&mut self.live_terminal) {
            self.mark_render_failure(error);
        }
    }

    pub(crate) fn take_submitted_prompt(&mut self) -> Option<String> {
        self.coordinator.take_submitted_prompt()
    }

    pub fn quit_requested(&self) -> bool {
        self.coordinator.quit_requested()
    }
}

impl<R, E> UiRenderer for TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    fn emit(&mut self, event: &UiEvent) {
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.enqueue_ui_event(event.clone());
        self.coordinator.drain_transport();
        if let Err(error) = self.coordinator.render_frame(&mut self.live_terminal) {
            self.mark_render_failure(error);
        }
        if !self.tui_active {
            self.inner.emit(event);
        }
    }

    fn flush(&mut self) {
        self.inner.flush();
    }
}

#[derive(Debug)]
pub enum RuntimeRunError<E> {
    Enter(TerminalLifecycleError),
    Run(RestoreRunError<E, TerminalLifecycleError>),
}

pub fn run_with_terminal_restore<B, T, E, F>(
    lifecycle: &mut TerminalLifecycle<B>,
    run: F,
) -> Result<T, RuntimeRunError<E>>
where
    B: TerminalBackend,
    F: FnOnce() -> Result<T, E>,
{
    lifecycle.enter().map_err(RuntimeRunError::Enter)?;
    run_with_restore(lifecycle, run).map_err(RuntimeRunError::Run)
}

pub struct AnsiTerminalBackend<W>
where
    W: Write,
{
    writer: W,
}

impl<W> AnsiTerminalBackend<W>
where
    W: Write,
{
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W> TerminalBackend for AnsiTerminalBackend<W>
where
    W: Write,
{
    fn enable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::terminal::enable_raw_mode().map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::EnableRawMode,
                err.to_string(),
            )
        })
    }

    fn disable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::terminal::disable_raw_mode().map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::DisableRawMode,
                err.to_string(),
            )
        })
    }

    fn enter_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::EnterAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::EnterAltScreen,
                err.to_string(),
            )
        })
    }

    fn leave_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::LeaveAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::LeaveAltScreen,
                err.to_string(),
            )
        })
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Hide).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::HideCursor,
                err.to_string(),
            )
        })
    }

    fn show_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Show).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::ShowCursor,
                err.to_string(),
            )
        })
    }
}

struct LiveTerminalUi {
    terminal: Terminal<CrosstermBackend<std::io::Stderr>>,
}

impl LiveTerminalUi {
    fn new() -> Result<Self, String> {
        let backend = CrosstermBackend::new(std::io::stderr());
        let mut terminal = Terminal::new(backend)
            .map_err(|err| format!("failed to initialize ratatui terminal: {err}"))?;
        terminal
            .clear()
            .map_err(|err| format!("failed to clear ratatui terminal: {err}"))?;
        Ok(Self { terminal })
    }
}
