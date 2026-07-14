use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error codes (A2A spec §9.5)
// ---------------------------------------------------------------------------

// Standard JSON-RPC error codes (RFC 4627)
pub const PARSE_ERROR: i32 = -32_700;
pub const INVALID_REQUEST: i32 = -32_600;
pub const METHOD_NOT_FOUND: i32 = -32_601;
pub const INVALID_PARAMS: i32 = -32_602;
pub const INTERNAL_ERROR: i32 = -32_603;

// A2A-specific error codes (spec §9.5)
pub const TASK_NOT_FOUND: i32 = -32_001;
pub const UNSUPPORTED_OPERATION: i32 = -32_002;
pub const CONTENT_TYPE_NOT_SUPPORTED: i32 = -32_003;
pub const TASK_ALREADY_EXISTS: i32 = -32_004;
pub const INVALID_TASK_STATE: i32 = -32_005;
pub const UNKNOWN_ERROR: i32 = -32_099;

// Error status strings (A2A spec §9.5)
pub const STATUS_TASK_NOT_FOUND: &str = "TASK_NOT_FOUND";
pub const STATUS_UNSUPPORTED_OPERATION: &str = "UNSUPPORTED_OPERATION";
pub const STATUS_CONTENT_TYPE_NOT_SUPPORTED: &str = "CONTENT_TYPE_NOT_SUPPORTED";
pub const STATUS_TASK_ALREADY_EXISTS: &str = "TASK_ALREADY_EXISTS";
pub const STATUS_INVALID_TASK_STATE: &str = "INVALID_TASK_STATE";
pub const STATUS_INTERNAL_ERROR: &str = "INTERNAL_ERROR";
pub const STATUS_UNKNOWN_ERROR: &str = "UNKNOWN_ERROR";

// ---------------------------------------------------------------------------
// A2aErrorResponse — error response body (A2A spec §9.5)
// ---------------------------------------------------------------------------

/// Top-level error response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aErrorResponse {
    pub error: A2aErrorBody,
}

/// Error body with code, status, message, and optional details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aErrorBody {
    pub code: u16,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ErrorDetail>,
}

/// A single error detail entry following the Google RPC error-info model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    #[serde(rename = "@type")]
    pub at_type: String,
    pub reason: String,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}
