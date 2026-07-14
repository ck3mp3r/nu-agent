use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::part::Part;
use super::role::Role;

// ---------------------------------------------------------------------------
// Message (A2A spec §4.6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}
