// region:    --- Modules

pub mod backend;
mod ui_renderer;

pub use ui_renderer::{RenderInput, UiRenderer};

// endregion: --- Modules

#[cfg(test)]
#[path = "../../test/renderer/contract.rs"]
mod contract_test;
#[cfg(test)]
#[path = "../../test/renderer/streaming/test.rs"]
mod streaming_test;

use agnt_kernel::bus::{LifecycleEvent, NarrationEvent};
use agnt_kernel::{
    CompactionRecord, CompactionSource, InferenceRecord, NoticeKind, PermissionRecord,
    SessionRecord, ToolRecord, ToolStatus,
};
use agnt_render::tool_args::plain_call_text;
use agnt_render::tool_view::ToolView;
use agnt_render::transcript::ir::{Display, StyleHint, annotate_diff_hint};
use std::io::Write;
use std::time::{Duration, Instant};

use crate::ansi::style_text;
use crate::formatter::{ToolEndView, format_tool_start};
use crate::markdown_buffer::StreamingMarkdownBuffer;
use crate::policy::{UiPolicy, Verbosity};
use crate::spinner::SpinnerState;

/// The label this drawer prints for a compaction's source.
///
/// The mapping is the drawer's: the record carries the enum, and this crate owns
/// the byte-for-byte text a terminal shows.
fn compaction_source_text(source: CompactionSource) -> &'static str {
    match source {
        CompactionSource::Auto => "auto",
        CompactionSource::Manual => "slash",
    }
}

/// The label this drawer prints for a policy resolution.
fn auto_resolution_text(action: agnt_kernel::AutoResolution) -> &'static str {
    match action {
        agnt_kernel::AutoResolution::Allow => "allow",
        agnt_kernel::AutoResolution::Deny => "deny",
    }
}

/// The label this drawer prints for a tool-error kind.
fn tool_error_text(kind: agnt_kernel::ToolErrorKind) -> &'static str {
    match kind {
        agnt_kernel::ToolErrorKind::InvalidArgs => "invalid_args",
        agnt_kernel::ToolErrorKind::Timeout => "timeout",
        agnt_kernel::ToolErrorKind::Cancelled => "cancelled",
        agnt_kernel::ToolErrorKind::NotFound => "not_found",
        agnt_kernel::ToolErrorKind::PermissionDenied => "permission_denied",
        agnt_kernel::ToolErrorKind::RateLimited => "rate_limited",
        agnt_kernel::ToolErrorKind::Provider => "provider",
        agnt_kernel::ToolErrorKind::Network => "network",
        agnt_kernel::ToolErrorKind::Other => "other",
        agnt_kernel::ToolErrorKind::Denied => "denied",
    }
}

/// The head of a compaction summary, as a terminal row shows it.
fn summary_preview(summary: &str) -> String {
    summary.chars().take(200).collect()
}

/// The call line the TTY prints for `call`: derived from the call's name and
/// raw arguments, with the arrow the terminal adds itself stripped.
fn call_line_for(call: &agnt_runtime::protocol::events::ToolCall) -> String {
    let line = call.call_line();
    plain_call_text(&line).to_string()
}

/// The label this renderer prints for a tool source.
trait ToolSourceLabel {
    fn label(self) -> &'static str;
}

impl ToolSourceLabel for agnt_runtime::tools::handler::ToolSource {
    fn label(self) -> &'static str {
        match self {
            Self::Closure => "closure",
            Self::Mcp => "mcp",
            Self::Builtin => "builtin",
            Self::Unknown => "unknown",
        }
    }
}

pub struct StderrUiRenderer<W: Write> {
    writer: W,
    policy: UiPolicy,
    spinner: SpinnerState,
    tick_gate: SystemTickGate,
    active_tool_name: Option<String>,
    active_tool_args: Option<String>,
    streaming_started: bool,
    streaming_printed_len: usize,
    /// The dispatched calls in the turn now running: the count the completion
    /// row reports. It folds from `ToolRecord::Started`, the same way the hook
    /// runner's turn fold counts them.
    tools_this_turn: usize,
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
            tools_this_turn: 0,
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
    fn render_tool_display(&mut self, display: &Display) {
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

    /// Flush whatever the markdown buffer still holds and clear the streaming
    /// cursor, so the next block starts fresh.
    fn reset_streaming(&mut self) {
        if self.streaming_started {
            let remaining = self.markdown_buffer.flush();
            if !remaining.is_empty() {
                let _ = self.writer.write_all(remaining.as_bytes());
            }
            let _ = self.writer.write_all(b"\n");
        }
        self.streaming_started = false;
        self.streaming_printed_len = 0;
        self.markdown_buffer.reset();
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

    fn render_input_line(&self, input: &RenderInput<'_>) -> Option<String> {
        match input {
            RenderInput::Record(SessionRecord::Tool(ToolRecord::Started { call })) => {
                if self.policy.quiet || self.spinner.is_enabled() {
                    None
                } else {
                    Some(format_tool_start(
                        self.policy.verbosity,
                        &call.name.0,
                        call.source.label(),
                        &call_line_for(call),
                    ))
                }
            }
            RenderInput::Record(SessionRecord::Tool(ToolRecord::Completed { call, outcome })) => {
                if self.policy.quiet {
                    None
                } else {
                    Some(
                        ToolEndView {
                            verbosity: self.policy.verbosity,
                            name: &call.name.0,
                            source: call.source.label(),
                            arguments: &call.arguments,
                            success: outcome.status == ToolStatus::Success,
                            result: &outcome.text,
                            error_kind: outcome.kind.map(tool_error_text),
                            message: outcome.message.as_deref(),
                        }
                        .format(),
                    )
                }
            }
            RenderInput::Record(SessionRecord::Permission(record)) => {
                if self.policy.quiet {
                    None
                } else {
                    self.permission_line(record)
                }
            }
            RenderInput::Record(SessionRecord::Compaction(record)) => {
                if self.policy.quiet {
                    None
                } else {
                    self.compaction_line(record)
                }
            }
            RenderInput::Record(SessionRecord::Turn(_)) => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!("✓ completed (tools={})", self.tools_this_turn))
                }
            }
            RenderInput::Narration(NarrationEvent::SystemMessage { kind, text }) => match kind {
                NoticeKind::Warning => {
                    if self.policy.verbosity >= Verbosity::VeryVerbose {
                        Some(format!("warning: {text}"))
                    } else {
                        None
                    }
                }
                NoticeKind::TurnError => Some(format!("Error: {text}")),
            },
            RenderInput::Narration(NarrationEvent::CompactionSummaryChunk { .. }) => None,
            RenderInput::Record(SessionRecord::Inference(_)) => None,
            RenderInput::Lifecycle(LifecycleEvent::CompactionStarted { source }) => {
                if self.policy.quiet {
                    None
                } else {
                    Some(style_text(
                        &format!(
                            "compaction: source={} status=running",
                            compaction_source_text(*source)
                        ),
                        &StyleHint::Muted,
                        self.use_color,
                    ))
                }
            }
            RenderInput::Lifecycle(_) => None,
            _ => None,
        }
    }

    /// The permission rows, field for field from the record the gate published.
    fn permission_line(&self, record: &PermissionRecord) -> Option<String> {
        match record {
            PermissionRecord::Requested {
                request_id,
                tool,
                source,
                rule_identity,
                summary,
                ..
            } => Some(format!(
                "permission requested: request_id={} tool={tool} source={source} rule={rule_identity} summary={summary}",
                request_id.as_str()
            )),
            PermissionRecord::Decided {
                request_id,
                decision,
                rule_identity,
            } => Some(format!(
                "permission decision: request_id={} decision={} rule={rule_identity}",
                request_id.as_str(),
                decision.as_str()
            )),
            PermissionRecord::TimedOut { request_id } => Some(format!(
                "permission timeout: request_id={} action=deny",
                request_id.as_str()
            )),
            PermissionRecord::Ignored { request_id, reason } => Some(format!(
                "permission decision ignored: request_id={} reason={reason}",
                request_id.as_str()
            )),
            PermissionRecord::Cancelled {
                request_id,
                rule_identity,
            } => Some(format!(
                "permission request cancelled: request_id={} rule={rule_identity}",
                request_id.as_str()
            )),
            PermissionRecord::AutoResolved {
                rule_identity,
                action,
                reason,
                ..
            } => Some(match reason {
                Some(reason) => format!(
                    "permission auto-resolved: rule={rule_identity} action={} reason={reason}",
                    auto_resolution_text(*action)
                ),
                None => format!(
                    "permission auto-resolved: rule={rule_identity} action={}",
                    auto_resolution_text(*action)
                ),
            }),
        }
    }

    /// The compaction rows. The preview is derived here: the record carries the
    /// whole summary, and a terminal row shows its head.
    fn compaction_line(&self, record: &CompactionRecord) -> Option<String> {
        match record {
            CompactionRecord::Requested { .. } | CompactionRecord::SummaryChunk { .. } => None,
            CompactionRecord::Started { source } => Some(style_text(
                &format!(
                    "compaction: source={} status=running",
                    compaction_source_text(*source)
                ),
                &StyleHint::Muted,
                self.use_color,
            )),
            CompactionRecord::Completed { source, summary } => Some(style_text(
                &format!(
                    "compaction: source={} preview={}",
                    compaction_source_text(*source),
                    summary_preview(summary)
                ),
                &StyleHint::Success,
                self.use_color,
            )),
            CompactionRecord::Failed { source, message } => Some(style_text(
                &format!(
                    "compaction: source={} status=failed message={message}",
                    compaction_source_text(*source)
                ),
                &StyleHint::Error,
                self.use_color,
            )),
            CompactionRecord::Uncompacted { source } => Some(style_text(
                &format!(
                    "compaction: source={} status=undone",
                    compaction_source_text(*source)
                ),
                &StyleHint::Muted,
                self.use_color,
            )),
        }
    }
}

impl<W: Write> UiRenderer for StderrUiRenderer<W> {
    fn draw(&mut self, input: RenderInput<'_>) {
        match &input {
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Started))
                if self.spinner.is_enabled() =>
            {
                self.active_tool_name = None;
                self.streaming_started = false;
                self.streaming_printed_len = 0;
                self.markdown_buffer.reset();
                self.spinner.start();
                self.draw_spinner();
            }
            RenderInput::Record(SessionRecord::Tool(ToolRecord::Started { call }))
                if self.spinner.is_enabled() && !self.policy.quiet =>
            {
                self.tools_this_turn += 1;
                self.active_tool_name = Some(call.name.0.clone());
                self.active_tool_args = Some(call_line_for(call));
                self.spinner.start();
                self.draw_spinner();
            }
            RenderInput::Narration(NarrationEvent::InferenceChunk { aggregated, .. }) => {
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
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Completed {
                ..
            }))
            | RenderInput::Record(SessionRecord::Tool(ToolRecord::Completed { .. }))
                if self.spinner.is_active() =>
            {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
            }
            RenderInput::Record(SessionRecord::Tool(ToolRecord::Completed { call, outcome }))
                if !self.policy.quiet =>
            {
                if let Some(display) = call.result_body(Some(&outcome.text)) {
                    self.render_tool_display(&display);
                }
            }
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Completed { usage }))
                if self.streaming_started =>
            {
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
            RenderInput::Record(SessionRecord::Turn(_)) if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
                self.reset_streaming();
            }
            RenderInput::Narration(NarrationEvent::SystemMessage {
                kind: NoticeKind::TurnError,
                ..
            }) if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
            }
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Stopped {
                reason,
                ..
            })) if reason.trim().is_empty() => {
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
                    self.reset_streaming();
                }
            }
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Stopped { .. }))
                if self.spinner.is_active() =>
            {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
            }
            RenderInput::Record(SessionRecord::Turn(_)) if self.streaming_started => {
                self.reset_streaming();
            }
            _ => {}
        }

        // The turn's own row is drawn from the fold, so the fold resets only
        // after that row has been rendered.
        let is_turn_end = matches!(input, RenderInput::Record(SessionRecord::Turn(_)));
        if let Some(line) = self.render_input_line(&input) {
            self.with_persistent_line(&line);
        }
        if is_turn_end {
            self.tools_this_turn = 0;
        }
    }

    fn flush(&mut self) {
        let _ = self.writer.flush();
    }

    fn tick(&mut self) {
        if self.spinner.is_active() && self.tick_gate.allow_tick() {
            self.spinner.tick();
            self.draw_spinner();
        }
    }
}
