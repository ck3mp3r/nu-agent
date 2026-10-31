// region:    --- Modules

pub mod backend;

// endregion: --- Modules

#[cfg(test)]
#[path = "../../test/renderer/contract.rs"]
mod contract_test;
#[cfg(test)]
#[path = "../../test/renderer/streaming/test.rs"]
mod streaming_test;

use agnt_core::protocol::event::ToolDisplay;
use agnt_core::protocol::events::{SystemEvent, SystemKind};
use agnt_core::renderer::UiRenderer;
use agnt_core::transcript::ir::{StyleHint, annotate_diff_hint};
use std::io::Write;
use std::time::{Duration, Instant};

use crate::ansi::style_text;
use crate::formatter::{ToolEndView, format_tool_start};
use crate::markdown_buffer::StreamingMarkdownBuffer;
use crate::policy::{UiPolicy, Verbosity};
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
            markdown_buffer: StreamingMarkdownBuffer::default(),
            use_color: stderr_is_tty,
        }
    }

    fn write_line(&mut self, line: &str) {
        let _ = self.writer.write_all(line.as_bytes());
        let _ = self.writer.write_all(b"\n");
    }

    /// Render a tool's diff/title/stats content with ANSI colors. Diff
    /// annotation routes through the shared `annotate_diff_hint` helper —
    /// the same function the markdown diff projection uses — so both paths
    /// style diff lines identically (hunk, file-meta, add, remove).
    fn render_tool_display(&mut self, display: &ToolDisplay) {
        if self.policy.quiet {
            return;
        }
        if let Some(title) = &display.title {
            let _ = self.writer.write_all(title.as_bytes());
            let _ = self.writer.write_all(b"\n");
        }
        for section in &display.sections {
            for line in section.content.lines() {
                let styled = style_text(line, &annotate_diff_hint(line), self.use_color);
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
                let line = if args.is_empty() {
                    format!("[{frame}] tool {tool_name}")
                } else {
                    format!("[{frame}] tool {tool_name} → {args}")
                };
                let _ = self.writer.write_all(line.as_bytes());
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

    fn render_event_line(&self, event: &SystemEvent) -> Option<String> {
        match event {
            SystemEvent::InferenceStarted => None,
            SystemEvent::Tick => None,
            SystemEvent::InferenceCompleted { .. } => None,
            SystemEvent::ToolStarted { call } => {
                if self.policy.quiet || self.spinner.is_enabled() {
                    None
                } else {
                    Some(format_tool_start(
                        self.policy.verbosity,
                        &call.name.0,
                        &call.source,
                        &call.call_line.to_plain_text(),
                    ))
                }
            }
            SystemEvent::ToolCompleted { call, outcome } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(
                        ToolEndView {
                            verbosity: self.policy.verbosity,
                            name: &call.name.0,
                            source: &call.source,
                            arguments: &call.arguments,
                            success: outcome.success,
                            result: &outcome.result,
                            error_kind: outcome.error_kind.as_deref(),
                            message: outcome.message.as_deref(),
                        }
                        .format(),
                    )
                }
            }
            SystemEvent::PreviewRequested { .. } => None,
            SystemEvent::PermissionRequested {
                request_id,
                context,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission requested: request_id={} tool={} source={} rule={} summary={}",
                        request_id.as_str(),
                        context.tool,
                        context.source,
                        context.matched_rule_identity,
                        context.summary
                    ))
                }
            }
            SystemEvent::PermissionDecisionSubmitted {
                request_id,
                decision,
                matched_rule_identity,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission decision: request_id={} decision={} rule={matched_rule_identity}",
                        request_id.as_str(),
                        decision.as_str()
                    ))
                }
            }
            SystemEvent::PermissionDecisionTimedOut { request_id } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission timeout: request_id={} action=deny",
                        request_id.as_str()
                    ))
                }
            }
            SystemEvent::PermissionDecisionIgnored { request_id, reason } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission decision ignored: request_id={} reason={reason}",
                        request_id.as_str()
                    ))
                }
            }
            SystemEvent::PermissionRequestCancelled {
                request_id,
                matched_rule_identity,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "permission request cancelled: request_id={} rule={matched_rule_identity}",
                        request_id.as_str()
                    ))
                }
            }
            SystemEvent::PermissionAutoResolved {
                matched_rule_identity,
                action,
                reason,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(match reason {
                        Some(reason) => format!(
                            "permission auto-resolved: rule={matched_rule_identity} action={} reason={reason}",
                            action.as_str()
                        ),
                        None => format!(
                            "permission auto-resolved: rule={matched_rule_identity} action={}",
                            action.as_str()
                        ),
                    })
                }
            }
            SystemEvent::SystemMessage { kind, text } => match kind {
                SystemKind::Warning => {
                    if self.policy.verbosity >= Verbosity::VeryVerbose {
                        Some(format!("warning: {text}"))
                    } else {
                        None
                    }
                }
                SystemKind::TurnError => Some(format!("Error: {text}")),
            },
            SystemEvent::CompactionStarted { source } => {
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
            SystemEvent::CompactionSummaryChunk { .. } => None,
            SystemEvent::CompactionCompleted {
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
            SystemEvent::CompactionFailed { source, message } => {
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
            SystemEvent::InferenceChunk { .. } => None,
            SystemEvent::InferenceStopped { reason } => {
                // An empty reason is a silent discard (provisional output from a
                // retried model turn), not a user-visible stop.
                if self.policy.quiet || reason.trim().is_empty() {
                    None
                } else {
                    Some(style_text(
                        &format!("⚠ stopped: {reason}"),
                        &StyleHint::Error,
                        self.use_color,
                    ))
                }
            }
            SystemEvent::TurnCompleted { tool_calls, .. } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!("✓ completed (tools={tool_calls})"))
                }
            }
            SystemEvent::KeyPressed { .. }
            | SystemEvent::WheelScrolled { .. }
            | SystemEvent::Resized { .. }
            | SystemEvent::PermissionDecided { .. }
            | SystemEvent::PromptSubmitted { .. }
            | SystemEvent::CompactionRequested { .. } => None,
        }
    }
}

impl<W: Write> UiRenderer for StderrUiRenderer<W> {
    fn emit(&mut self, event: &SystemEvent) {
        match event {
            SystemEvent::InferenceStarted if self.spinner.is_enabled() => {
                self.active_tool_name = None;
                self.streaming_started = false;
                self.streaming_printed_len = 0;
                self.markdown_buffer.reset();
                self.spinner.start();
                self.draw_spinner();
            }
            SystemEvent::ToolStarted { call }
                if self.spinner.is_enabled() && !self.policy.quiet =>
            {
                self.active_tool_name = Some(call.name.0.clone());
                self.active_tool_args = Some(call.call_line.to_plain_text());
                self.spinner.start();
                self.draw_spinner();
            }
            SystemEvent::InferenceChunk { aggregated, .. } => {
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
                // Buffer the not-yet-printed portion and emit only the safe
                // markdown prefix.
                if aggregated.len() > self.streaming_printed_len {
                    let delta = &aggregated[self.streaming_printed_len..];
                    self.streaming_printed_len += delta.len();
                    let safe = self.markdown_buffer.push(delta);
                    if !safe.is_empty() {
                        let _ = self.writer.write_all(safe.as_bytes());
                        let _ = self.writer.flush();
                    }
                }
            }
            SystemEvent::Tick if self.spinner.is_active() && self.tick_gate.allow_tick() => {
                self.spinner.tick();
                self.draw_spinner();
                return;
            }
            SystemEvent::InferenceCompleted { .. } | SystemEvent::ToolCompleted { .. }
                if self.spinner.is_active() =>
            {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
            }
            SystemEvent::ToolCompleted { outcome, .. } if !self.policy.quiet => {
                if let Some(display) = &outcome.display {
                    self.render_tool_display(display);
                }
            }
            SystemEvent::InferenceCompleted { usage } if self.streaming_started => {
                let remaining = self.markdown_buffer.flush();
                if !remaining.is_empty() {
                    let _ = self.writer.write_all(remaining.as_bytes());
                }
                let _ = self.writer.write_all(b"\n");
                if !self.policy.quiet
                    && let Some(total_tokens) = usage.total_tokens.filter(|total| *total > 0)
                {
                    let line = format!(
                        "  {total_tokens} tokens ({} in + {} out)",
                        usage.input_tokens, usage.output_tokens
                    );
                    let styled = style_text(&line, &StyleHint::Muted, self.use_color);
                    self.write_line(&styled);
                }
                self.streaming_started = false;
                self.streaming_printed_len = 0;
                self.markdown_buffer.reset();
            }
            SystemEvent::TurnCompleted { .. } if self.spinner.is_active() => {
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
            SystemEvent::SystemMessage {
                kind: SystemKind::TurnError,
                ..
            } if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
            }
            SystemEvent::InferenceStopped { reason } if reason.trim().is_empty() => {
                // Silent discard of provisional output from a retried model
                // turn: reset the streaming state so the retry's text starts a
                // fresh block instead of being sliced against the discarded
                // provisional length.
                if self.spinner.is_active() {
                    self.clear_spinner_line();
                    self.spinner.stop();
                    self.active_tool_name = None;
                }
                if self.streaming_started {
                    let _ = self.writer.write_all(b"\n");
                    self.streaming_started = false;
                    self.streaming_printed_len = 0;
                    self.markdown_buffer.reset();
                }
            }
            SystemEvent::InferenceStopped { .. } if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
            }
            SystemEvent::TurnCompleted { .. } if self.streaming_started => {
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
