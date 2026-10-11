use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// SendMessageConfiguration (A2A spec §3.2.2)
// ---------------------------------------------------------------------------

/// Per-request configuration for `message:send` (spec §3.2.2).
///
/// `return_immediately` selects the response mode: absent or `false` means
/// blocking (the server waits for a terminal state), `true` means the server
/// returns as soon as the task is created.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_immediately: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_output_modes: Option<Vec<String>>,
}

impl SendMessageConfiguration {
    /// Returns `true` when the request asks for a non-blocking response.
    ///
    /// Absent `return_immediately` means blocking (spec §3.2.2).
    pub fn is_return_immediately(&self) -> bool {
        self.return_immediately.unwrap_or(false)
    }
}
