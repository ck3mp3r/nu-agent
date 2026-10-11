use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

mod support;
use support::*;

mod channels;
mod context_parent;
mod extensions;
mod files;
mod history;
mod idempotency;
mod list_tasks;
mod metadata;
mod peer_cache;
mod push_notifications;
mod server_lifecycle;
mod streaming;
mod tasks_get_cancel;
mod tasks_send;
mod version_enforcement;
