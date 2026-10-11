use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Role (A2A spec §4.6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

impl Role {
    /// Human-readable label for display contexts (TUI, logs).
    pub fn label(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Agent => "agent",
        }
    }
}
