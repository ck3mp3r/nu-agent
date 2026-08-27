#[cfg(test)]
mod contract_test;
#[cfg(test)]
mod streaming_test;
pub mod tty;
#[cfg(test)]
mod tty_test;

use nu_agent_core::policy::{UiPolicy, Verbosity};
use nu_agent_core::protocol::event::{ToolDisplay, UiEvent};
use nu_agent_core::renderer::UiRenderer;
use nu_agent_core::transcript::ir::StyleHint;
use std::io::Write;
use std::time::{Duration, Instant};

use crate::ansi::style_text;
use crate::formatter::{ToolEndView, format_tool_start};
use crate::markdown_buffer::StreamingMarkdownBuffer;
use crate::spinner::SpinnerState;

pub struct StderrUiRenderer<W: Write> {
    writer: W,
    policy: UiPolicy,
    spinner: SpinnerState,
    tick_gate: SystemTickGate,
    active_tool_name: Option<String>,
    active_tool_args: Option<String>,
    streaming_started: bool,
    streaming_printed_len: usize,
    markdown_buffer: StreamingMarkdownBuffer,
    use_color: bool,
}

trait TickGate {
    fn allow_tick(&mut self) -> bool;
}

#[derive(Debug, Clone)]
struct SystemTickGate {
    interval: Duration,
    last_tick: Option<Instant>,
}

impl SystemTickGate {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_tick: None,
        }
    }
}

impl TickGate for SystemTickGate {
    fn allow_tick(&mut self) -> bool {
        let now = Instant::now();
        match self.last_tick {
            None => {
                self.last_tick = Some(now);
                true
            }
            Some(last) if now.duration_since(last) >= self.interval => {
                self.last_tick = Some(now);
                true
            }
            Some(_) => false,
        }
    }
}

impl<W: Write> StderrUiRenderer<W> {
    pub fn new(writer: W, policy: UiPolicy, stderr_is_tty: bool) -> Self {
        let spinner_enabled = stderr_is_tty && policy.allows_spinner();
        Self {
            writer,
            policy,
            spinner: SpinnerState::new(spinner_enabled),
            tick_gate: SystemTickGate::new(Duration::from_millis(80)),
            active_tool_name: None,
            active_tool_args: None,
            streaming_started: false,
            streaming_printed_len: 0,
            markdown_buffer: StreamingMarkdownBuffer::new(),
            use_color: stderr_is_tty,
        }
    }

    fn write_line(&mut self, line: &str) {
        let _ = self.writer.write_all(line.as_bytes());
        let _ = self.writer.write_all(b"\n");
    }

    /// Render a tool's diff/title/stats content with ANSI colors.
    fn render_tool_display(&mut self, display: &ToolDisplay) {
        if self.policy.quiet {
            return;
        }
        let _ = self.writer.write_all(display.title.as_bytes());
        let _ = self.writer.write_all(b"\n");
        for section in &display.sections {
            for line in section.content.lines() {
                let hint = if line.starts_with('+') {
                    StyleHint::DiffAdd
                } else if line.starts_with('-') {
                    StyleHint::DiffRemove
                } else {
                    StyleHint::Normal
                };
                let styled = style_text(line, &hint, self.use_color);
                self.write_line(&styled);
            }
            if let Some(stats) = &section.stats {
                let mut parts = Vec::new();
                if let Some(f) = stats.files_changed {
                    parts.push(format!("{f} files changed"));
                }
                if let Some(i) = stats.insertions {
                    parts.push(format!("{i} insertions"));
                }
                if let Some(d) = stats.deletions {
                    parts.push(format!("{d} deletions"));
                }
                if let Some(o) = stats.omitted_files {
                    parts.push(format!("{o} files omitted"));
                }
                if !parts.is_empty() {
                    let line = parts.join(", ");
                    let styled = style_text(&line, &StyleHint::Muted, self.use_color);
                    self.write_line(&styled);
                }
            }
        }
    }

    fn clear_spinner_line(&mut self) {
        let _ = self.writer.write_all(b"\r\x1b[2K");
    }

    fn draw_spinner(&mut self) {
        if self.spinner.is_enabled() && self.spinner.is_active() {
            self.clear_spinner_line();
            let frame = self.spinner.current_frame();
            if let Some(tool_name) = &self.active_tool_name {
                let args = self.active_tool_args.as_deref().unwrap_or("{}");
                let _ = self
                    .writer
                    .write_all(format!("[{frame}] tool {tool_name} → {args}").as_bytes());
            } else {
                let _ = self.writer.write_all(frame.as_bytes());
            }
        }
    }

    fn with_persistent_line(&mut self, line: &str) {
        let was_active = self.spinner.is_active();
        if was_active {
            self.spinner.suspend();
            self.clear_spinner_line();
        }
        self.write_line(line);
        if was_active {
            self.spinner.resume();
            self.draw_spinner();
        }
    }

    fn render_event_line(&self, event: &UiEvent) -> Option<String> {
        match event {
            UiEvent::LlmStarted => None,
            UiEvent::Tick => None,
            UiEvent::LlmCompleted {
                response_chars,
                tool_calls,
                ..
            } => {
                let _ = (response_chars, tool_calls);
                None
            }
            UiEvent::ToolStarted {
                name,
                source,
                arguments,
            } => {
                if self.policy.quiet || self.spinner.is_enabled() {
                    None
                } else {
                    Some(format_tool_start(
                        self.policy.verbosity,
                        name,
                        source,
                        arguments,
                    ))
                }
            }
            UiEvent::ToolCompleted {
                name,
                source,
                arguments,
                success,
                result,
                display: _,
                error_kind,
                message,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(
                        ToolEndView {
                            verbosity: self.policy.verbosity,
                            name,
                            source,
                            arguments,
                            success: *success,
                            result,
                            error_kind: error_kind.as_deref(),
                            message: message.as_deref(),
                        }
                        .format(),
                    )
                }
            }
            UiEvent::PermissionRequested {
                request_id,
                context,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission requested: request_id={request_id} tool={} source={} rule={} summary={}",
                        context.tool,
                        context.source,
                        context.matched_rule_identity,
                        context.summary
                    ))
                }
            }
            UiEvent::PermissionDecisionSubmitted {
                request_id,
                decision,
                matched_rule_identity,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission decision: request_id={request_id} decision={} rule={matched_rule_identity}",
                        decision.as_str()
                    ))
                }
            }
            UiEvent::PermissionDecisionTimedOut { request_id } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission timeout: request_id={request_id} action=deny"
                    ))
                }
            }
            UiEvent::PermissionDecisionIgnored { request_id, reason } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission decision ignored: request_id={request_id} reason={reason}"
                    ))
                }
            }
            UiEvent::Warning { message } => {
                if self.policy.verbosity >= Verbosity::VeryVerbose {
                    Some(format!("warning: {message}"))
                } else {
                    None
                }
            }
            UiEvent::TurnError { message } => Some(format!("Error: {}", message)),
            UiEvent::CompactionStarted { source } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(style_text(
                        &format!("compaction: source={source} status=running"),
                        &StyleHint::Muted,
                        self.use_color,
                    ))
                }
            }
            UiEvent::CompactionSummaryChunk { .. } => None,
            UiEvent::CompactionCompleted {
                source,
                summary_preview,
                ..
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(style_text(
                        &format!("compaction: source={source} preview={summary_preview}"),
                        &StyleHint::Success,
                        self.use_color,
                    ))
                }
            }
            UiEvent::CompactionFailed { source, message } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(style_text(
                        &format!("compaction: source={source} status=failed message={message}"),
                        &StyleHint::Error,
                        self.use_color,
                    ))
                }
            }
            UiEvent::AssistantMessage { .. } => None,
            UiEvent::Completed { tool_calls } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!("✓ completed (tools={tool_calls})"))
                }
            }
        }
    }
}

impl<W: Write> UiRenderer for StderrUiRenderer<W> {
    fn emit(&mut self, event: &UiEvent) {
        match event {
            UiEvent::LlmStarted if self.spinner.is_enabled() => {
                self.active_tool_name = None;
                self.streaming_started = false;
                self.streaming_printed_len = 0;
                self.markdown_buffer.reset();
                self.spinner.start();
                self.draw_spinner();
            }
            UiEvent::ToolStarted { name, .. }
                if self.spinner.is_enabled() && !self.policy.quiet =>
            {
                self.active_tool_name = Some(name.clone());
                if let UiEvent::ToolStarted { arguments, .. } = event {
                    self.active_tool_args = Some(arguments.clone());
                }
                self.spinner.start();
                self.draw_spinner();
            }
            UiEvent::AssistantMessage { text } => {
                if self.policy.quiet {
                    return;
                }
                if !self.streaming_started {
                    self.streaming_started = true;
                    // Stop the spinner when streaming starts
                    if self.spinner.is_active() {
                        self.clear_spinner_line();
                        self.spinner.stop();
                    }
                }
                // Buffer the new portion and emit only the safe markdown prefix.
                if text.len() > self.streaming_printed_len {
                    let delta = &text[self.streaming_printed_len..];
                    self.streaming_printed_len = text.len();
                    let safe = self.markdown_buffer.push(delta);
                    if !safe.is_empty() {
                        let _ = self.writer.write_all(safe.as_bytes());
                        let _ = self.writer.flush();
                    }
                }
            }
            UiEvent::Tick if self.spinner.is_active() && self.tick_gate.allow_tick() => {
                self.spinner.tick();
                self.draw_spinner();
                return;
            }
            UiEvent::LlmCompleted { .. } | UiEvent::ToolCompleted { .. }
                if self.spinner.is_active() =>
            {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
            }
            UiEvent::ToolCompleted {
                display: Some(display),
                ..
            } if !self.policy.quiet => {
                self.render_tool_display(display);
            }
            UiEvent::LlmCompleted {
                input_tokens,
                output_tokens,
                total_tokens,
                ..
            } if self.streaming_started => {
                let remaining = self.markdown_buffer.flush();
                if !remaining.is_empty() {
                    let _ = self.writer.write_all(remaining.as_bytes());
                }
                let _ = self.writer.write_all(b"\n");
                if !self.policy.quiet && *total_tokens > 0 {
                    let line = format!(
                        "  {total_tokens} tokens ({input_tokens} in + {output_tokens} out)"
                    );
                    let styled = style_text(&line, &StyleHint::Muted, self.use_color);
                    self.write_line(&styled);
                }
                self.streaming_started = false;
                self.streaming_printed_len = 0;
                self.markdown_buffer.reset();
            }
            UiEvent::Completed { .. } if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
                if self.streaming_started {
                    let remaining = self.markdown_buffer.flush();
                    if !remaining.is_empty() {
                        let _ = self.writer.write_all(remaining.as_bytes());
                    }
                    let _ = self.writer.write_all(b"\n");
                    self.streaming_started = false;
                    self.streaming_printed_len = 0;
                    self.markdown_buffer.reset();
                }
            }
            UiEvent::TurnError { .. } if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
            }
            UiEvent::Completed { .. } if self.streaming_started => {
                let remaining = self.markdown_buffer.flush();
                if !remaining.is_empty() {
                    let _ = self.writer.write_all(remaining.as_bytes());
                }
                let _ = self.writer.write_all(b"\n");
                self.streaming_started = false;
                self.streaming_printed_len = 0;
                self.markdown_buffer.reset();
            }
            _ => {}
        }

        if let Some(line) = self.render_event_line(event) {
            self.with_persistent_line(&line);
        }
    }

    fn flush(&mut self) {
        let _ = self.writer.flush();
    }
}
