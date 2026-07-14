use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Push notification configs
// ---------------------------------------------------------------------------

/// A webhook push notification configuration for a task.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushNotificationConfig {
    pub id: String,
    pub url: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<PushAuthenticationInfo>,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
}

/// Authentication information for a push notification webhook.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushAuthenticationInfo {
    #[serde(flatten)]
    pub scheme: PushAuthScheme,
}

/// Authentication scheme for push notification webhooks.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "scheme")]
pub enum PushAuthScheme {
    Bearer {
        token: String,
    },
    Basic {
        username: String,
        password: String,
    },
    Custom {
        name: String,
        #[serde(rename = "credentials")]
        credentials: serde_json::Value,
    },
}
