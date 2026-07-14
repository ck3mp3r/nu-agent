use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Role (A2A spec §4.6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    User,
    Agent,
}
