use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    style::{Modifier, Style},
    layout::Position,
    layout::{Constraint, Direction, Layout},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use crossterm::cursor::SetCursorStyle;

use crate::commands::agent::ui::{
    event::UiEvent,
    renderer::UiRenderer,
    tui::{
        cancel::CancelController,
        dispatch::dispatch_terminal_event,
        input::{TerminalEvent, TerminalKey},
        layout::{
            INPUT_MIN_HEIGHT, TRANSCRIPT_MIN_HEIGHT, LayoutInput, LayoutOutput,
            input_cursor_row_col, input_pane_height_for_content, recompute_layout,
            wrapped_input_rows,
        },
        markdown::project_markdown_to_lines,
        reducer::{ReducerInput, reduce_with_cancel_controller},
        selection::TranscriptSelection,
        state::{AppState, PromptStatus, ToolCallStatus, TranscriptLineStatus, TranscriptRole},
        terminal::{TerminalBackend, TerminalLifecycle, TerminalLifecycleError},
        theme::TuiTheme,
        transport::TuiTransport,
    },
};
use crate::commands::agent::contracts::UiMessageSnapshot;

use crate::commands::agent::ui::tui::safety::{RestoreRunError, run_with_restore};

pub trait TerminalEventSource {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String>;

    fn diagnostics(&self) -> InputSourceDiagnostics {
        InputSourceDiagnostics::unknown()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSourceDiagnostics {
    pub active_backend: &'static str,
    pub primary_available: Option<bool>,
    pub fallback_available: Option<bool>,
    pub last_poll_state: String,
    pub last_error: Option<String>,
}

impl InputSourceDiagnostics {
    fn unknown() -> Self {
        Self {
            active_backend: "unknown",
            primary_available: None,
            fallback_available: None,
            last_poll_state: "waiting for input poll".to_string(),
            last_error: None,
        }
    }
}

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
    const HEADER_HEIGHT: u16 = 1;
    const STATUS_TARGET_HEIGHT: u16 = 1;

    pub fn new(columns: u16, rows: u16, side_pane_visible: Option<bool>) -> Self {
        Self::new_with_watchdog(
            columns,
            rows,
            side_pane_visible,
            Self::DEFAULT_INPUT_WATCHDOG_TIMEOUT,
        )
    }

    pub fn hydrate_transcript_from_messages(
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
                for line in project_markdown_to_lines(message_content) {
                    let plain_text = crate::commands::agent::ui::tui::markdown::rendered_line_to_plain_text(&line);
                    if !plain_text.trim().is_empty() {
                        self.state.push_transcript_rendered_line(role, line);
                    }
                }
                continue;
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

    pub fn take_submitted_prompt(&mut self) -> Option<String> {
        self.state.take_next_prompt_for_execution()
    }

    pub fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.active_model_identity = active_model_identity;
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn fatal_error(&self) -> Option<&str> {
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
            && self.state.phase == crate::commands::agent::ui::tui::state::UiPhase::Idle
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
                let transcript = window_lines
                .into_iter()
                .enumerate()
                .flat_map(|(offset, line)| {
                    let global_idx = window_start.saturating_add(offset);
                    let line_status = self.state.transcript_line_status_for_index(global_idx);
                    let is_cursor_line = self.state.transcript_cursor_index() == Some(global_idx)
                        && self.state.input_mode
                            != crate::commands::agent::ui::tui::state::InputMode::Insert;
                    let is_selected = selected
                        .map(|(start, end)| global_idx >= start && global_idx <= end)
                        .unwrap_or(false);
                    render_transcript_lines(
                        line,
                        vertical[1].width as usize,
                        is_selected,
                        is_cursor_line,
                        line_status,
                        current_time_millis(),
                        &self.theme,
                    )
                })
                .collect::<Vec<_>>();
                let transcript_view_height = vertical[1].height.saturating_sub(1) as usize;
                let _transcript_title = transcript_title_for_render(
                    &self.state,
                    self.state.transcript_preview.len(),
                );
                let transcript_border_style = if self.state.pane_focus
                    == crate::commands::agent::ui::tui::state::PaneFocus::Transcript
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
                    == crate::commands::agent::ui::tui::state::PaneFocus::Input
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

    fn vertical_heights_for_main(main_height: u16) -> (u16, u16, u16, u16) {
        Self::vertical_heights_for_main_with_input(main_height, INPUT_MIN_HEIGHT)
    }

    fn vertical_heights_for_main_with_input(main_height: u16, input_target_height: u16) -> (u16, u16, u16, u16) {
        if main_height == 0 {
            return (0, 0, 0, 0);
        }

        let header = Self::HEADER_HEIGHT.min(main_height);
        let mut remaining = main_height.saturating_sub(header);

        let input = input_target_height.max(INPUT_MIN_HEIGHT).min(remaining);
        remaining = remaining.saturating_sub(input);

        let min_transcript = u16::from(remaining > 0).min(TRANSCRIPT_MIN_HEIGHT);
        let status = Self::STATUS_TARGET_HEIGHT.min(remaining.saturating_sub(min_transcript));
        let transcript = remaining.saturating_sub(status);

        (header, transcript, status, input)
    }

    fn transcript_height_for_main(main_height: u16) -> u16 {
        let (_, transcript, _, _) = Self::vertical_heights_for_main(main_height);
        transcript
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
        let main = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 120,
            height: main_height,
        };
        let (header, transcript, status, input) = Self::vertical_heights_for_main(main.height);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header),
                Constraint::Length(transcript),
                Constraint::Length(input),
                Constraint::Length(status),
            ])
            .split(main);
        (vertical[0], vertical[1], vertical[2], vertical[3])
    }

    #[cfg(test)]
    pub fn pump_once(&mut self, event_source: &mut impl TerminalEventSource) {
        self.poll_terminal_event(event_source);
        self.drain_transport();
    }
}

fn build_status_lines(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> Vec<String> {
    let status = if state.status_line.is_empty() {
        match state.phase {
            crate::commands::agent::ui::tui::state::UiPhase::Idle => {
                "Idle (type and press Enter)"
            }
            crate::commands::agent::ui::tui::state::UiPhase::Busy => "Thinking...",
            crate::commands::agent::ui::tui::state::UiPhase::AbortPending => {
                crate::commands::agent::ui::tui::reducer::ESC_ABORT_CONFIRM_STATUS
            }
        }
    } else {
        &state.status_line
    };

    let input_error = last_input_error.unwrap_or("none");
    let tokens_line = format_tokens_line(state);
    let mode_line = match state.input_mode {
        crate::commands::agent::ui::tui::state::InputMode::Insert => {
            "Mode: INSERT (typing · Esc/jj/jk -> NORMAL)".to_string()
        }
        crate::commands::agent::ui::tui::state::InputMode::Normal => {
            "Mode: NORMAL (navigation · i INSERT · v VISUAL · h/l or Tab pane · j/k · gg/G)"
                .to_string()
        }
        crate::commands::agent::ui::tui::state::InputMode::Visual => {
            "Mode: VISUAL (transcript selection · j/k · gg/G · y yank · Esc)"
                .to_string()
        }
    };
    let focus_line = match state.pane_focus {
        crate::commands::agent::ui::tui::state::PaneFocus::Transcript => {
            "Focus: Transcript".to_string()
        }
        crate::commands::agent::ui::tui::state::PaneFocus::Input => "Focus: Input".to_string(),
    };
    let mut lines = vec![
        status.to_string(),
        mode_line,
        focus_line,
    ];

    if let Some(visual_line) = visual_indicator_line(state) {
        lines.push(visual_line);
    }

    lines.extend([
        tokens_line,
        format!("Model: {active_model_identity}"),
        format!("Input backend: {input_backend_status}"),
        format!("Input poll: {last_input_poll_status}"),
        format!("Input error: {input_error}"),
    ]);

    lines
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

fn compact_status_line(
    state: &AppState,
    active_model_identity: &str,
    _input_backend_status: &str,
    _last_input_poll_status: &str,
    _last_input_error: Option<&str>,
) -> String {
    let status = if state.status_line.is_empty() {
        match state.phase {
            crate::commands::agent::ui::tui::state::UiPhase::Idle => "",
            crate::commands::agent::ui::tui::state::UiPhase::Busy => "busy",
            crate::commands::agent::ui::tui::state::UiPhase::AbortPending => "abort-pending",
        }
    } else {
        &state.status_line
    };

    let mode = match state.input_mode {
        crate::commands::agent::ui::tui::state::InputMode::Insert => "INS",
        crate::commands::agent::ui::tui::state::InputMode::Normal => "NOR",
        crate::commands::agent::ui::tui::state::InputMode::Visual => "VIS",
    };

    let tokens = match (
        state.latest_input_tokens,
        state.latest_output_tokens,
        state.latest_total_tokens,
    ) {
        (_, _, Some(_)) => format!("tokens: {}", state.session_total_tokens),
        _ => "tokens: n/a".to_string(),
    };

    let queue = state.pending_prompt_count();

    let mut parts = Vec::new();
    if !status.is_empty() {
        parts.push(status.to_string());
    }
    parts.push(mode.to_string());
    parts.push(format!("queue: {queue}"));
    parts.push(tokens);
    parts.push(active_model_identity.to_string());

    parts.join(" | ")
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

fn transcript_selection_for_render(state: &AppState) -> Option<TranscriptSelection> {
    if state.input_mode != crate::commands::agent::ui::tui::state::InputMode::Visual {
        return None;
    }
    if state.pane_focus != crate::commands::agent::ui::tui::state::PaneFocus::Transcript {
        return None;
    }

    let (Some(anchor), Some(cursor)) = (state.visual_anchor_index(), state.visual_cursor_index()) else {
        return None;
    };

    let mut selection = TranscriptSelection::new(anchor);
    selection.set_cursor(cursor);
    Some(selection)
}

fn transcript_selection_range_for_render(
    state: &AppState,
    transcript_len: usize,
) -> Option<(usize, usize)> {
    transcript_selection_for_render(state).and_then(|selection| selection.bounded_range(transcript_len))
}

fn transcript_title_for_render(state: &AppState, transcript_len: usize) -> String {
    let Some(selection) = transcript_selection_for_render(state) else {
        return "Transcript".to_string();
    };

    match selection.bounded_range(transcript_len) {
        Some((start, end)) => format!(
            "Transcript [VISUAL anchor={} cursor={} range={}..{}]",
            selection.anchor(),
            selection.cursor(),
            start,
            end
        ),
        None => "Transcript [VISUAL]".to_string(),
    }
}

fn visual_indicator_line(state: &AppState) -> Option<String> {
    let selection = transcript_selection_for_render(state)?;
    let (start, end) = selection.normalized_range();
    Some(format!(
        "Visual: transcript anchor={} cursor={} range={}..{}",
        selection.anchor(),
        selection.cursor(),
        start,
        end
    ))
}

fn cursor_style_for_mode(
    mode: crate::commands::agent::ui::tui::state::InputMode,
) -> SetCursorStyle {
    match mode {
        crate::commands::agent::ui::tui::state::InputMode::Insert => SetCursorStyle::SteadyBar,
        crate::commands::agent::ui::tui::state::InputMode::Normal
        | crate::commands::agent::ui::tui::state::InputMode::Visual => SetCursorStyle::SteadyBlock,
    }
}

fn format_tokens_line(state: &AppState) -> String {
    match (
        state.latest_input_tokens,
        state.latest_output_tokens,
        state.latest_total_tokens,
    ) {
        (Some(input), Some(output), Some(total)) => format!(
            "Tokens: in={input} out={output} total={total} session={}",
            state.session_total_tokens
        ),
        _ => "Tokens: n/a".to_string(),
    }
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
    mode: crate::commands::agent::ui::tui::state::InputMode,
) -> SetCursorStyle {
    cursor_style_for_mode(mode)
}

fn availability_label(availability: Option<bool>) -> &'static str {
    match availability {
        Some(true) => "available",
        Some(false) => "unavailable",
        None => "unknown",
    }
}

#[cfg(test)]
pub(super) fn visible_transcript_window(
    transcript: &[crate::commands::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
) -> Vec<crate::commands::agent::ui::tui::state::TranscriptLine> {
    let (_, lines) = visible_transcript_window_with_start(
        transcript,
        visible_lines,
        scroll_from_bottom,
        follow_tail,
    );
    lines
}

fn visible_transcript_window_with_start(
    transcript: &[crate::commands::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
) -> (usize, Vec<crate::commands::agent::ui::tui::state::TranscriptLine>) {
    let total_lines = transcript.len();
    if visible_lines == 0 || total_lines == 0 {
        return (0, Vec::new());
    }

    if total_lines <= visible_lines {
        return (0, transcript.to_vec());
    }

    let max_from_bottom = total_lines - visible_lines;
    let from_bottom = if follow_tail {
        0
    } else {
        scroll_from_bottom.min(max_from_bottom)
    };

    let start = total_lines - visible_lines - from_bottom;
    let end = (start + visible_lines).min(total_lines);
    (start, transcript[start..end].to_vec())
}

fn visible_transcript_window_for_render(
    transcript: &[crate::commands::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
) -> (usize, Vec<crate::commands::agent::ui::tui::state::TranscriptLine>) {
    if !follow_tail {
        return visible_transcript_window_with_start(
            transcript,
            visible_lines,
            scroll_from_bottom,
            follow_tail,
        );
    }

    fit_tail_window_by_wrapped_rows(transcript, visible_lines, content_width)
}

#[cfg(test)]
pub(super) fn visible_transcript_window_for_render_for_test(
    transcript: &[crate::commands::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
) -> (usize, Vec<crate::commands::agent::ui::tui::state::TranscriptLine>) {
    visible_transcript_window_for_render(
        transcript,
        visible_lines,
        scroll_from_bottom,
        follow_tail,
        content_width,
    )
}

fn fit_tail_window_by_wrapped_rows(
    transcript: &[crate::commands::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    content_width: usize,
) -> (usize, Vec<crate::commands::agent::ui::tui::state::TranscriptLine>) {
    let total_lines = transcript.len();
    if total_lines == 0 || visible_lines == 0 {
        return (0, Vec::new());
    }

    let width = content_width.max(1);
    let mut start = total_lines;
    let mut used_rows = 0usize;

    for idx in (0..total_lines).rev() {
        let rows = wrapped_row_count_for_line(&transcript[idx], width);
        if used_rows.saturating_add(rows) > visible_lines {
            if start == total_lines {
                start = idx;
            }
            break;
        }

        used_rows = used_rows.saturating_add(rows);
        start = idx;
    }

    if start == total_lines {
        start = total_lines.saturating_sub(1);
    }

    (start, transcript[start..total_lines].to_vec())
}

fn wrapped_row_count_for_line(
    line: &crate::commands::agent::ui::tui::state::TranscriptLine,
    content_width: usize,
) -> usize {
    if content_width == 0 {
        return 0;
    }

    if line.role == TranscriptRole::Separator {
        return 1;
    }

    let prefix_width = 2usize;
    let available = content_width.max(prefix_width).saturating_sub(prefix_width).max(1);
    let chars = line.text.chars().count();
    chars.max(1).div_ceil(available)
}

fn transcript_role_style(role: TranscriptRole) -> Style {
    let theme = TuiTheme::default();
    match role {
        TranscriptRole::User => theme.role_user,
        TranscriptRole::Assistant => theme.role_assistant,
        TranscriptRole::System => theme.role_system,
        TranscriptRole::Tool => theme.role_tool,
        TranscriptRole::Separator => theme.role_separator,
    }
}

fn render_transcript_lines(
    line: crate::commands::agent::ui::tui::state::TranscriptLine,
    content_width: usize,
    selected: bool,
    cursor_line: bool,
    line_status: Option<TranscriptLineStatus>,
    now_millis: u128,
    theme: &TuiTheme,
) -> Vec<Line<'static>> {
    let selection_overlay = if selected {
        theme.selection_bg
    } else {
        Style::default()
    };
    let cursor_prefix = if cursor_line { "> " } else { "  " };
    if line.role == TranscriptRole::Separator {
        let width = content_width.saturating_sub(2).max(1);
        let desired = line
            .text
            .chars()
            .next()
            .map(|ch| ch.to_string().repeat(width))
            .unwrap_or_else(|| "-".repeat(width));
        let style = transcript_role_style(TranscriptRole::Separator);
        return vec![Line::from(vec![
            Span::styled(cursor_prefix.to_string(), style.patch(selection_overlay)),
            Span::styled(desired, style.patch(selection_overlay)),
        ])];
    }

    let role_style = transcript_role_style(line.role);
    let indicator = line_status
        .map(|status| indicator_for_line_status(status, now_millis))
        .unwrap_or("");
    let prompt_modifier = if line_status
        == Some(TranscriptLineStatus::Prompt(PromptStatus::Cancelled))
    {
        theme.cancelled_modifier
    } else {
        Modifier::empty()
    };

    let prefix = if indicator.is_empty() {
        cursor_prefix.to_string()
    } else {
        format!("{cursor_prefix}{indicator} ")
    };

    if let Some(rendered) = line.rendered {
        let mut spans = vec![Span::styled(
            prefix,
            role_style.patch(selection_overlay).add_modifier(prompt_modifier),
        )];
        spans.extend(
            rendered
            .spans
            .into_iter()
            .map(|span| {
                let style = span.style.patch(role_style).add_modifier(prompt_modifier);
                Span::styled(span.content.into_owned(), style.patch(selection_overlay))
            })
            .collect::<Vec<_>>(),
        );
        return vec![Line::from(spans)];
    }

    vec![Line::from(vec![
        Span::styled(prefix, role_style.patch(selection_overlay).add_modifier(prompt_modifier)),
        Span::styled(
            line.text,
            role_style.patch(selection_overlay).add_modifier(prompt_modifier),
        ),
    ])]
}

const IN_PROGRESS_SPINNER_FRAMES: [&str; 10] = [
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
];

fn prompt_indicator_for_status(status: PromptStatus, now_millis: u128) -> &'static str {
    match status {
        PromptStatus::Queued => "•",
        PromptStatus::InProgress => {
            let idx = ((now_millis / 100) % IN_PROGRESS_SPINNER_FRAMES.len() as u128) as usize;
            IN_PROGRESS_SPINNER_FRAMES[idx]
        }
        PromptStatus::Done => "✓",
        PromptStatus::Cancelled => "✕",
    }
}

fn tool_indicator_for_status(status: ToolCallStatus, now_millis: u128) -> &'static str {
    match status {
        ToolCallStatus::InProgress => {
            let idx = ((now_millis / 100) % IN_PROGRESS_SPINNER_FRAMES.len() as u128) as usize;
            IN_PROGRESS_SPINNER_FRAMES[idx]
        }
        ToolCallStatus::Done => "✓",
        ToolCallStatus::Failed => "✕",
    }
}

fn indicator_for_line_status(status: TranscriptLineStatus, now_millis: u128) -> &'static str {
    match status {
        TranscriptLineStatus::Prompt(prompt) => prompt_indicator_for_status(prompt, now_millis),
        TranscriptLineStatus::Tool(tool) => tool_indicator_for_status(tool, now_millis),
    }
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
    line: crate::commands::agent::ui::tui::state::TranscriptLine,
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

fn current_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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

    pub fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.coordinator
            .set_active_model_identity(active_model_identity);
    }

    pub fn fatal_error(&self) -> Option<&str> {
        self.coordinator.fatal_error()
    }

    pub fn hydrate_transcript_from_messages(
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

    pub fn take_submitted_prompt(&mut self) -> Option<String> {
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
                crate::commands::agent::ui::tui::terminal::TerminalAction::EnableRawMode,
                err.to_string(),
            )
        })
    }

    fn disable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::terminal::disable_raw_mode().map_err(|err| {
            TerminalLifecycleError::new(
                crate::commands::agent::ui::tui::terminal::TerminalAction::DisableRawMode,
                err.to_string(),
            )
        })
    }

    fn enter_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::EnterAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::commands::agent::ui::tui::terminal::TerminalAction::EnterAltScreen,
                err.to_string(),
            )
        })
    }

    fn leave_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::LeaveAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::commands::agent::ui::tui::terminal::TerminalAction::LeaveAltScreen,
                err.to_string(),
            )
        })
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Hide).map_err(|err| {
            TerminalLifecycleError::new(
                crate::commands::agent::ui::tui::terminal::TerminalAction::HideCursor,
                err.to_string(),
            )
        })
    }

    fn show_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Show).map_err(|err| {
            TerminalLifecycleError::new(
                crate::commands::agent::ui::tui::terminal::TerminalAction::ShowCursor,
                err.to_string(),
            )
        })
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ScriptedTerminalEvents {
    queue: std::collections::VecDeque<TerminalEvent>,
}

#[cfg(test)]
impl ScriptedTerminalEvents {
    pub fn from_script(script: &str) -> Self {
        let mut queue = std::collections::VecDeque::new();

        for raw in script.split(',') {
            let token = raw.trim();
            if token.is_empty() {
                continue;
            }

            if let Some(event) = parse_script_token(token) {
                queue.push_back(event);
            }
        }

        Self { queue }
    }
}

#[cfg(test)]
impl TerminalEventSource for ScriptedTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        Ok(self.queue.pop_front())
    }
}

#[derive(Debug, Clone)]
pub struct CrosstermTerminalEvents {
    poll_timeout: Duration,
}

impl Default for CrosstermTerminalEvents {
    fn default() -> Self {
        Self::new(Duration::from_millis(60))
    }
}

impl CrosstermTerminalEvents {
    pub fn new(poll_timeout: Duration) -> Self {
        Self { poll_timeout }
    }
}

impl TerminalEventSource for CrosstermTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        let ready = crossterm::event::poll(self.poll_timeout)
            .map_err(|err| format!("crossterm poll failed: {err}"))?;
        if !ready {
            return Ok(None);
        }

        let event = crossterm::event::read().map_err(|err| format!("crossterm read failed: {err}"))?;
        Ok(map_crossterm_event(event))
    }
}

#[derive(Debug)]
pub struct HybridTerminalEvents {
    primary: CrosstermTerminalEvents,
    fallback: Option<TtyTerminalEvents>,
    diagnostics: InputSourceDiagnostics,
}

impl HybridTerminalEvents {
    pub fn new(poll_timeout: Duration, fallback: Option<TtyTerminalEvents>) -> Self {
        let fallback_available = Some(fallback.is_some());
        Self {
            primary: CrosstermTerminalEvents::new(poll_timeout),
            fallback,
            diagnostics: InputSourceDiagnostics {
                active_backend: "none",
                primary_available: Some(true),
                fallback_available,
                last_poll_state: "not polled yet".to_string(),
                last_error: None,
            },
        }
    }
}

fn poll_hybrid_event<P, F>(
    primary: &mut P,
    mut fallback: Option<&mut F>,
    diagnostics: &mut InputSourceDiagnostics,
    prefix_fallback_idle_error: bool,
) -> Result<Option<TerminalEvent>, String>
where
    P: TerminalEventSource,
    F: TerminalEventSource,
{
    match primary.poll_event() {
        Ok(Some(event)) => {
            diagnostics.active_backend = "crossterm";
            diagnostics.primary_available = Some(true);
            diagnostics.last_poll_state = "crossterm delivered event".to_string();
            diagnostics.last_error = None;
            Ok(Some(event))
        }
        Ok(None) => match fallback.as_mut() {
            Some(fallback) => match fallback.poll_event() {
                Ok(Some(event)) => {
                    diagnostics.active_backend = "tty";
                    diagnostics.fallback_available = Some(true);
                    diagnostics.last_poll_state = "/dev/tty delivered event".to_string();
                    diagnostics.last_error = None;
                    Ok(Some(event))
                }
                Ok(None) => {
                    diagnostics.active_backend = "none";
                    diagnostics.last_poll_state = "crossterm idle; /dev/tty idle".to_string();
                    Ok(None)
                }
                Err(fallback_error) => {
                    diagnostics.active_backend = "none";
                    diagnostics.fallback_available = Some(false);
                    diagnostics.last_poll_state = "crossterm idle; /dev/tty error".to_string();
                    diagnostics.last_error = Some(fallback_error.clone());
                    if prefix_fallback_idle_error {
                        Err(format!("tty fallback failed: {fallback_error}"))
                    } else {
                        Err(fallback_error)
                    }
                }
            },
            None => {
                diagnostics.active_backend = "none";
                diagnostics.fallback_available = Some(false);
                diagnostics.last_poll_state = "crossterm idle; /dev/tty unavailable".to_string();
                Ok(None)
            }
        },
        Err(primary_error) => match fallback.as_mut() {
            Some(fallback) => match fallback.poll_event() {
                Ok(Some(event)) => {
                    diagnostics.active_backend = "tty";
                    diagnostics.primary_available = Some(false);
                    diagnostics.fallback_available = Some(true);
                    diagnostics.last_poll_state =
                        "crossterm error; /dev/tty delivered event".to_string();
                    diagnostics.last_error = Some(primary_error);
                    Ok(Some(event))
                }
                Ok(None) => {
                    diagnostics.active_backend = "none";
                    diagnostics.primary_available = Some(false);
                    diagnostics.last_poll_state = "crossterm error; /dev/tty idle".to_string();
                    diagnostics.last_error = Some(primary_error.clone());
                    Ok(None)
                }
                Err(fallback_error) => {
                    diagnostics.active_backend = "none";
                    diagnostics.primary_available = Some(false);
                    diagnostics.fallback_available = Some(false);
                    diagnostics.last_poll_state = "crossterm error; /dev/tty error".to_string();
                    diagnostics.last_error =
                        Some(format!("{primary_error}; tty fallback failed: {fallback_error}"));
                    Err(format!("{primary_error}; tty fallback failed: {fallback_error}"))
                }
            },
            None => {
                diagnostics.active_backend = "none";
                diagnostics.primary_available = Some(false);
                diagnostics.fallback_available = Some(false);
                diagnostics.last_poll_state = "crossterm error; /dev/tty unavailable".to_string();
                diagnostics.last_error = Some(primary_error.clone());
                Err(primary_error)
            }
        },
    }
}

#[cfg(test)]
pub(crate) struct HybridTerminalEventsForTest<P, F>
where
    P: TerminalEventSource,
    F: TerminalEventSource,
{
    primary: P,
    fallback: F,
    diagnostics: InputSourceDiagnostics,
}

#[cfg(test)]
impl HybridTerminalEvents {
    pub(crate) fn new_for_test<P, F>(
        primary: P,
        fallback: F,
    ) -> HybridTerminalEventsForTest<P, F>
    where
        P: TerminalEventSource,
        F: TerminalEventSource,
    {
        HybridTerminalEventsForTest {
            primary,
            fallback,
            diagnostics: InputSourceDiagnostics {
                active_backend: "none",
                primary_available: Some(true),
                fallback_available: Some(true),
                last_poll_state: "not polled yet".to_string(),
                last_error: None,
            },
        }
    }
}

#[cfg(test)]
impl<P, F> TerminalEventSource for HybridTerminalEventsForTest<P, F>
where
    P: TerminalEventSource,
    F: TerminalEventSource,
{
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        poll_hybrid_event(
            &mut self.primary,
            Some(&mut self.fallback),
            &mut self.diagnostics,
            false,
        )
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

impl TerminalEventSource for HybridTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        poll_hybrid_event(
            &mut self.primary,
            self.fallback.as_mut(),
            &mut self.diagnostics,
            true,
        )
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

#[derive(Debug)]
pub struct TtyTerminalEvents {
    reader: File,
}

impl TtyTerminalEvents {
    pub fn new(tty_reader: File, _poll_timeout: Duration) -> Result<Self, String> {
        set_nonblocking(&tty_reader)?;
        Ok(Self { reader: tty_reader })
    }
}

impl TerminalEventSource for TtyTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        let mut buf = [0u8; 1];
        match self.reader.read(&mut buf) {
            Ok(0) => Err("/dev/tty returned EOF".to_string()),
            Ok(_) => Ok(map_tty_byte(buf[0])),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => Ok(None),
            Err(err) if err.raw_os_error() == Some(5) => Ok(None),
            Err(err) => Err(format!("/dev/tty read failed: {err}")),
        }
    }
}

fn set_nonblocking(file: &File) -> Result<(), String> {
    let fd = file.as_raw_fd();
    // SAFETY: fcntl is called with valid fd and standard F_GETFL/F_SETFL commands.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(format!(
            "failed to query /dev/tty file status flags: {}",
            std::io::Error::last_os_error()
        ));
    }

    // SAFETY: same as above; we set O_NONBLOCK while preserving existing flags.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if rc < 0 {
        return Err(format!(
            "failed to enable non-blocking mode for /dev/tty: {}",
            std::io::Error::last_os_error()
        ));
    }

    Ok(())
}

fn map_tty_byte(byte: u8) -> Option<TerminalEvent> {
    let key = match byte {
        3 => TerminalKey::CtrlC,
        b'\r' | b'\n' => TerminalKey::Enter,
        8 | 127 => TerminalKey::Backspace,
        27 => TerminalKey::Esc,
        value if value.is_ascii_graphic() || value == b' ' => TerminalKey::Char(value as char),
        _ => return None,
    };

    Some(TerminalEvent::Key(key))
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

pub fn open_tty_reader() -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .open("/dev/tty")
        .map_err(|err| format!("failed to open /dev/tty for reading: {err}"))
}

fn map_crossterm_event(event: crossterm::event::Event) -> Option<TerminalEvent> {
    use crossterm::event::{
        Event,
        KeyCode,
        KeyEventKind,
        KeyModifiers,
    };

    match event {
        Event::Resize(columns, rows) => {
            Some(TerminalEvent::Resize(crate::commands::agent::ui::tui::input::TerminalResize {
                columns,
                rows,
            }))
        }
        Event::Key(key_event) => {
            if key_event.kind != KeyEventKind::Press && key_event.kind != KeyEventKind::Repeat {
                return None;
            }

            let key = match key_event.code {
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlC
                }
                KeyCode::Char('u') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlU
                }
                KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlD
                }
                KeyCode::Char(ch) => TerminalKey::Char(ch),
                KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    TerminalKey::AltEnter
                }
                KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
                    TerminalKey::ShiftEnter
                }
                KeyCode::Enter => TerminalKey::Enter,
                KeyCode::Backspace => TerminalKey::Backspace,
                KeyCode::Delete => TerminalKey::Delete,
                KeyCode::Left => TerminalKey::Left,
                KeyCode::Right => TerminalKey::Right,
                KeyCode::Home => TerminalKey::Home,
                KeyCode::End => TerminalKey::End,
                KeyCode::Up => TerminalKey::Up,
                KeyCode::Down => TerminalKey::Down,
                KeyCode::PageUp => TerminalKey::PageUp,
                KeyCode::PageDown => TerminalKey::PageDown,
                KeyCode::Tab => TerminalKey::Tab,
                KeyCode::BackTab => TerminalKey::BackTab,
                KeyCode::Esc => TerminalKey::Esc,
                _ => return None,
            };

            Some(TerminalEvent::Key(key))
        }
        _ => None,
    }
}

#[cfg(test)]
pub(super) fn map_crossterm_event_for_test(event: crossterm::event::Event) -> Option<TerminalEvent> {
    map_crossterm_event(event)
}

#[cfg(test)]
fn parse_script_token(token: &str) -> Option<TerminalEvent> {
    use crate::commands::agent::ui::tui::input::TerminalResize;

    let lower = token.to_ascii_lowercase();
    let key = match lower.as_str() {
        "enter" => Some(TerminalKey::Enter),
        "backspace" => Some(TerminalKey::Backspace),
        "delete" => Some(TerminalKey::Delete),
        "left" => Some(TerminalKey::Left),
        "right" => Some(TerminalKey::Right),
        "home" => Some(TerminalKey::Home),
        "end" => Some(TerminalKey::End),
        "up" => Some(TerminalKey::Up),
        "down" => Some(TerminalKey::Down),
        "pgup" | "pageup" => Some(TerminalKey::PageUp),
        "pgdown" | "pagedown" => Some(TerminalKey::PageDown),
        "tab" => Some(TerminalKey::Tab),
        "backtab" => Some(TerminalKey::BackTab),
        "esc" => Some(TerminalKey::Esc),
        "ctrlc" => Some(TerminalKey::CtrlC),
        "ctrlu" => Some(TerminalKey::CtrlU),
        "ctrld" => Some(TerminalKey::CtrlD),
        _ => None,
    };

    if let Some(key) = key {
        return Some(TerminalEvent::Key(key));
    }

    if let Some(chars) = token.strip_prefix("char:") {
        return chars
            .chars()
            .next()
            .map(TerminalKey::Char)
            .map(TerminalEvent::Key);
    }

    if let Some(size) = token.strip_prefix("resize:")
        && let Some((columns, rows)) = size.split_once('x')
        && let (Ok(columns), Ok(rows)) = (columns.parse::<u16>(), rows.parse::<u16>())
    {
        return Some(TerminalEvent::Resize(TerminalResize { columns, rows }));
    }

    None
}
