//! Doom loop detection concern.
//!
//! Detects when the agent calls the same tool with identical arguments
//! repeatedly, indicating an infinite loop.

use std::sync::{Arc, Mutex};

use rig::agent::ToolCallAction;
use serde_json::Value;

use crate::bus::{Bus, WarningEvent};

pub const DOOM_LOOP_THRESHOLD: usize = 5;
pub const DOOM_LOOP_BACKOFF_LIMIT: usize = 2;
pub const DOOM_LOOP_STOP_PREFIX: &str = "Doom loop stopped:";

/// Tracks recent tool call signatures for doom loop detection.
#[derive(Debug, Clone, Default)]
pub struct DoomLoopState {
    pub(crate) recent_signatures: Vec<(String, String)>, // (tool_name, arguments)
    pub(crate) escalation_count: usize,
}

/// The escalation level of a doom-loop detection within one turn attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoomLoopDetection {
    /// First detection in this turn attempt: steer the model.
    First(String),
    /// Backoff detection in this turn attempt: stronger steering naming the tool.
    Backoff(String),
    /// Stop detection in this turn attempt: stop the run.
    Stop(String),
}

impl DoomLoopState {
    /// Clears accumulated signatures and the escalation counter, resetting
    /// doom loop detection.
    pub fn reset(&mut self) {
        self.recent_signatures.clear();
        self.escalation_count = 0;
    }

    /// Returns the detection with the looping tool name if a doom loop is
    /// detected, incrementing the escalation counter per turn attempt.
    pub(crate) fn check_and_record(&mut self, name: &str, args: &str) -> Option<DoomLoopDetection> {
        let canonical_args = canonicalize_args(args);
        self.recent_signatures
            .push((name.to_string(), canonical_args));

        if self.recent_signatures.len() < DOOM_LOOP_THRESHOLD {
            return None;
        }

        let last_n = &self.recent_signatures[self.recent_signatures.len() - DOOM_LOOP_THRESHOLD..];
        let first = &last_n[0];
        if !last_n.iter().all(|sig| sig == first) {
            return None;
        }

        self.escalation_count += 1;
        let detection = if self.escalation_count == 1 {
            DoomLoopDetection::First(name.to_string())
        } else if self.escalation_count <= 1 + DOOM_LOOP_BACKOFF_LIMIT {
            DoomLoopDetection::Backoff(name.to_string())
        } else {
            DoomLoopDetection::Stop(name.to_string())
        };
        Some(detection)
    }
}

// region:    --- Support

/// Normalizes tool-call arguments for signature comparison.
///
/// Parses the args as JSON and re-serializes a canonical form: object keys
/// sorted lexicographically at every nesting level, array element order
/// preserved, compact output. Falls back to the raw string when the args are
/// not valid JSON so non-JSON signatures stay byte-exact.
fn canonicalize_args(args: &str) -> String {
    match serde_json::from_str::<Value>(args) {
        Ok(value) => canonical_value(&value),
        Err(_) => args.to_string(),
    }
}

/// Recursively canonicalizes a JSON value.
///
/// Object keys are sorted lexicographically (serde_json builds with
/// `preserve_order` transitively, so `Value::to_string()` does not sort).
/// Array element order is significant and preserved.
fn canonical_value(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_else(|_| key.clone()),
                        canonical_value(&map[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(items) => {
            let elements = items
                .iter()
                .map(canonical_value)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{elements}]")
        }
        other => other.to_string(),
    }
}

// endregion: --- Support

/// Wraps shared [`DoomLoopState`] with detection logic.
#[derive(Clone)]
pub struct DoomLoopDetector {
    pub state: Arc<Mutex<DoomLoopState>>,
}

impl DoomLoopDetector {
    /// Check for a doom loop and record the tool call signature.
    ///
    /// Returns `Some(ToolCallAction::skip(...))` with a steering message on the
    /// first and backoff detections in a turn attempt, `Some(ToolCallAction::stop(...))`
    /// on the stop detection, `None` otherwise.
    /// Using `Skip` feeds the message to the LLM as a tool result so it can
    /// change course; `Stop` is the backstop for a model that ignores the
    /// steering.
    pub async fn check_and_record(
        &self,
        tool_name: &str,
        args: &str,
        bus: &Bus,
    ) -> Option<ToolCallAction> {
        let detected = {
            let mut state = self.state.lock().expect("doom loop mutex poisoned");
            state.check_and_record(tool_name, args)
        };
        match detected {
            Some(DoomLoopDetection::First(tool)) => {
                log::warn!("Doom loop detected: tool={tool_name}");
                let message = format!(
                    "Doom loop detected: '{}' called {} times with identical arguments. \
                     Are you really sure you need so many tool calls? Reconsider your approach.",
                    tool, DOOM_LOOP_THRESHOLD
                );
                let _ = bus
                    .warning()
                    .send(WarningEvent::Message {
                        message: message.clone(),
                    })
                    .await;
                Some(ToolCallAction::skip(message))
            }
            Some(DoomLoopDetection::Backoff(tool)) => {
                log::warn!("Doom loop backoff: tool={tool_name}");
                let message = format!(
                    "Doom loop persisted: '{}' triggered repeated loop detections. \
                     Change your approach: use different arguments, a different tool, or ask the user for guidance.",
                    tool
                );
                let _ = bus
                    .warning()
                    .send(WarningEvent::Message {
                        message: message.clone(),
                    })
                    .await;
                Some(ToolCallAction::skip(message))
            }
            Some(DoomLoopDetection::Stop(tool)) => {
                log::warn!("Doom loop stopped: tool={tool_name}");
                let message = format!(
                    "{DOOM_LOOP_STOP_PREFIX} '{}' kept looping after repeated steering. The run was stopped.",
                    tool
                );
                Some(ToolCallAction::stop(message))
            }
            None => None,
        }
    }
}

#[cfg(test)]
#[path = "doom_loop_test.rs"]
mod doom_loop_test;
