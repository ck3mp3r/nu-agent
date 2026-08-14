//! Doom loop detection concern.
//!
//! Detects when the agent calls the same tool with identical arguments
//! repeatedly, indicating an infinite loop.

use std::sync::{Arc, Mutex};

use rig::agent::ToolCallAction;

use crate::bus::{Bus, WarningEvent};

pub const DOOM_LOOP_THRESHOLD: usize = 5;

/// Tracks recent tool call signatures for doom loop detection.
#[derive(Debug, Clone, Default)]
pub struct DoomLoopState {
    pub(crate) recent_signatures: Vec<(String, String)>, // (tool_name, arguments)
}

impl DoomLoopState {
    /// Clears accumulated signatures, resetting doom loop detection.
    pub fn reset(&mut self) {
        self.recent_signatures.clear();
    }

    /// Returns `Some(tool_name)` if a doom loop is detected.
    pub(crate) fn check_and_record(&mut self, name: &str, args: &str) -> Option<String> {
        self.recent_signatures
            .push((name.to_string(), args.to_string()));

        if self.recent_signatures.len() < DOOM_LOOP_THRESHOLD {
            return None;
        }

        let last_n = &self.recent_signatures[self.recent_signatures.len() - DOOM_LOOP_THRESHOLD..];
        let first = &last_n[0];
        if last_n.iter().all(|sig| sig == first) {
            Some(name.to_string())
        } else {
            None
        }
    }
}

/// Wraps shared [`DoomLoopState`] with detection logic.
#[derive(Clone)]
pub struct DoomLoopDetector {
    pub state: Arc<Mutex<DoomLoopState>>,
}

impl DoomLoopDetector {
    /// Check for a doom loop and record the tool call signature.
    ///
    /// Returns `Some(ToolCallAction::skip(...))` with a detection message if a doom loop is detected, `None` otherwise.
    /// Using `Skip` feeds the message to the LLM as a tool result so it can change course.
    pub fn check_and_record(
        &self,
        tool_name: &str,
        args: &str,
        bus: &Bus,
    ) -> Option<ToolCallAction> {
        let mut state = self.state.lock().expect("doom loop mutex poisoned");
        if let Some(tool) = state.check_and_record(tool_name, args) {
            log::warn!("Doom loop detected: tool={tool_name}");
            let message = format!(
                "Doom loop detected: '{}' called {} times with identical arguments",
                tool, DOOM_LOOP_THRESHOLD
            );
            let _ = bus.warning().send(WarningEvent::Message {
                message: message.clone(),
            });
            Some(ToolCallAction::skip(message))
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "doom_loop_test.rs"]
mod doom_loop_test;
