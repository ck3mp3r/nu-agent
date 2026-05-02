use std::io::Write;
use std::time::{Duration, Instant};

use super::{
    event::UiEvent,
    formatter::{ToolEndView, format_tool_end, format_tool_start},
    policy::UiPolicy,
    renderer::UiRenderer,
    spinner::SpinnerState,
};

pub struct StderrUiRenderer<W: Write> {
    writer: W,
    policy: UiPolicy,
    spinner: SpinnerState,
    tick_gate: SystemTickGate,
    active_tool_name: Option<String>,
    active_tool_args: Option<String>,
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
                    Some(format_tool_start(self.policy.verbosity, name, source, arguments))
                }
            }
            UiEvent::ToolEnd {
                name,
                source,
                arguments,
                success,
                result,
                error_kind,
                message,
            } => {
                if self.policy.quiet {
                    None
                } else {
                    Some(format_tool_end(
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
                    ))
                }
            }
            UiEvent::Warning { message } => Some(format!("warning: {message}")),
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
    pub(super) fn spinner_enabled_for_test(&self) -> bool {
        self.spinner.is_enabled()
    }

    #[cfg(test)]
    pub(super) fn spinner_active_for_test(&self) -> bool {
        self.spinner.is_active()
    }

    #[cfg(test)]
    pub(super) fn spinner_suspended_for_test(&self) -> bool {
        self.spinner.is_suspended()
    }

    #[cfg(test)]
    pub(super) fn spinner_frame_for_test(&self) -> &str {
        self.spinner.current_frame()
    }
}

impl<W: Write> UiRenderer for StderrUiRenderer<W> {
    fn emit(&mut self, event: &UiEvent) {
        match event {
            UiEvent::LlmStart if self.spinner.is_enabled() => {
                self.active_tool_name = None;
                self.spinner.start();
                self.draw_spinner();
            }
            UiEvent::ToolStart { name, .. }
                if self.spinner.is_enabled() && !self.policy.quiet =>
            {
                self.active_tool_name = Some(name.clone());
                if let UiEvent::ToolStart { arguments, .. } = event {
                    self.active_tool_args = Some(arguments.clone());
                }
                self.spinner.start();
                self.draw_spinner();
            }
            UiEvent::Tick if self.spinner.is_active() && self.tick_gate.allow_tick() => {
                self.spinner.tick();
                self.draw_spinner();
                return;
            }
            UiEvent::LlmEnd { .. }
                | UiEvent::ToolEnd { .. }
                | UiEvent::Completed { .. }
                if self.spinner.is_active() =>
            {
                self.clear_spinner_line();
                self.spinner.stop();
                self.active_tool_name = None;
                self.active_tool_args = None;
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
