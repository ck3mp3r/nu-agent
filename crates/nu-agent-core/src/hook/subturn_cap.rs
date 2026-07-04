//! Per-sub-turn tool call cap concern.
//!
//! Limits the number of tool calls per LLM sub-turn (one LLM response cycle).
//! The counter is reset by [`SubTurnCap::reset`] when a new LLM request fires.

use std::sync::{Arc, Mutex};

use rig::agent::ToolCallHookAction;

/// Enforces a per-sub-turn tool call cap.
///
/// When `max == 0`, the cap is disabled and all calls pass through.
#[derive(Clone)]
pub struct SubTurnCap {
    pub max: usize,
    pub count: Arc<Mutex<usize>>,
}

impl SubTurnCap {
    /// Create a new cap with the given maximum (0 = unlimited).
    pub fn new(max: usize) -> Self {
        Self {
            max,
            count: Arc::new(Mutex::new(0)),
        }
    }

    /// Reset the counter — call this at the start of each new sub-turn.
    pub fn reset(&self) {
        *self.count.lock().expect("subturn cap mutex poisoned") = 0;
    }

    /// Check and increment the counter.
    ///
    /// Returns `Some(Skip)` if the cap is exceeded, `None` if the call should proceed.
    pub fn check_and_increment(&self, tool_name: &str) -> Option<ToolCallHookAction> {
        if self.max == 0 {
            return None;
        }
        let mut count = self.count.lock().expect("subturn cap mutex poisoned");
        if *count >= self.max {
            log::warn!(
                "Tool call cap: tool={tool_name} count={} max={}",
                *count,
                self.max
            );
            return Some(ToolCallHookAction::Skip {
                reason: "Tool call limit exceeded for this sub-turn. Remaining calls skipped."
                    .to_string(),
            });
        }
        *count += 1;
        None
    }
}

#[cfg(test)]
#[path = "subturn_cap_test.rs"]
mod subturn_cap_test;
