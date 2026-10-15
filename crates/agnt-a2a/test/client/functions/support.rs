//! Shared fixtures for the client function test modules.
//!
//! Holds the imports and the `Result` alias every topical client function test
//! file needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::client::{
    A2aClient, MockHttpClient, cancel_task, get_agent_card, get_task, list_tasks, send_task,
};
pub(super) use crate::{A2aError, Message, Part, Role, SendMessageConfiguration, TaskState};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
