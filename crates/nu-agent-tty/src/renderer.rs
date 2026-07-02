use nu_agent_core::policy::{UiPolicy, Verbosity};
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::renderer::UiRenderer;
use std::io::Write;
use std::time::{Duration, Instant};

use crate::formatter::{ToolEndView, format_tool_start};
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
        }
    }

    fn write_line(&mut self, line: &str) {
        let _ = self.writer.write_all(line.as_bytes());
        let _ = self.writer.write_all(b"\n");
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
                    .write_all(format!("[{frame}] tool {tool_name} args={args}").as_bytes());
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
            UiEvent::LlmStart => None,
            UiEvent::Tick => None,
            UiEvent::LlmEnd {
                response_chars,
                tool_calls,
                ..
            } => {
                let _ = (response_chars, tool_calls);
                None
            }
            UiEvent::ToolStart {
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
            UiEvent::ToolEnd {
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
                    Some(format!("compaction: source={source} status=running"))
                }
            }
            UiEvent::CompactionSummaryChunk { .. } => None,
            UiEvent::CompactionTriggered {
                source,
                summarized_count,
                summary_preview,
                ..
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "compaction: source={source} summarized={summarized_count} preview={summary_preview}"
                    ))
                }
            }
            UiEvent::CompactionFailed { source, message } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format!(
                        "compaction: source={source} status=failed message={message}"
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

    #[cfg(test)]
    pub(crate) fn spinner_enabled_for_test(&self) -> bool {
        self.spinner.is_enabled()
    }

    #[cfg(test)]
    pub(crate) fn spinner_active_for_test(&self) -> bool {
        self.spinner.is_active()
    }

    #[cfg(test)]
    pub(crate) fn spinner_suspended_for_test(&self) -> bool {
        self.spinner.is_suspended()
    }

    #[cfg(test)]
    pub(crate) fn spinner_frame_for_test(&self) -> &str {
        self.spinner.current_frame()
    }
}

impl<W: Write> UiRenderer for StderrUiRenderer<W> {
    fn emit(&mut self, event: &UiEvent) {
        match event {
            UiEvent::LlmStart if self.spinner.is_enabled() => {
                self.active_tool_name = None;
                self.streaming_started = false;
                self.streaming_printed_len = 0;
                self.spinner.start();
                self.draw_spinner();
            }
            UiEvent::ToolStart { name, .. } if self.spinner.is_enabled() && !self.policy.quiet => {
                self.active_tool_name = Some(name.clone());
                if let UiEvent::ToolStart { arguments, .. } = event {
                    self.active_tool_args = Some(arguments.clone());
                }
                self.spinner.start();
                self.draw_spinner();
            }
            UiEvent::AssistantMessage { text } => {
                // Only stream output in verbose mode (-v or higher)
                if self.policy.verbosity >= Verbosity::Verbose {
                    if !self.streaming_started {
                        self.streaming_started = true;
                        // Stop the spinner when streaming starts
                        if self.spinner.is_active() {
                            self.clear_spinner_line();
                            self.spinner.stop();
                        }
                    }
                    // Only print the new portion (text is accumulated, not delta)
                    if text.len() > self.streaming_printed_len {
                        let _ = self
                            .writer
                            .write_all(&text.as_bytes()[self.streaming_printed_len..]);
                        self.streaming_printed_len = text.len();
                    }
                }
            }
            UiEvent::Tick if self.spinner.is_active() && self.tick_gate.allow_tick() => {
                self.spinner.tick();
                self.draw_spinner();
                return;
            }
            UiEvent::LlmEnd { .. } | UiEvent::ToolEnd { .. } if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
            }
            UiEvent::Completed { .. } if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
                if self.streaming_started {
                    let _ = self.writer.write_all(b"\n");
                    self.streaming_started = false;
                    self.streaming_printed_len = 0;
                }
            }
            UiEvent::TurnError { .. } if self.spinner.is_active() => {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
            }
            UiEvent::Completed { .. } if self.streaming_started => {
                let _ = self.writer.write_all(b"\n");
                self.streaming_started = false;
                self.streaming_printed_len = 0;
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
