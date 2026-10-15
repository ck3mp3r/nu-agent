//! Shared fixtures for the in-memory task store test modules.
//!
//! Holds the imports and the `Result` alias every topical store test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::task_store::{
    InMemoryTaskStore, TaskStoreBackend, is_valid_transition, task_event_to_stream_response,
};
pub(super) use crate::{
    A2aError, Artifact, Message, Part, PushAuthScheme, PushAuthenticationInfo, Role, TaskEvent,
    TaskState, TaskStatus,
};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
