//! Shared test-support types for kernel contract tests.

use std::fmt;

/// The concrete test-error type. Static dispatch all the way — no `Box<dyn
/// Error>` in kernel tests.
#[derive(Debug)]
pub struct TestError(String);

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TestError {}

impl From<&str> for TestError {
    fn from(message: &str) -> Self {
        Self(message.to_string())
    }
}

impl From<String> for TestError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<std::num::ParseIntError> for TestError {
    fn from(error: std::num::ParseIntError) -> Self {
        Self(format!("parse error: {error}"))
    }
}

impl From<crate::bus::PublishError> for TestError {
    fn from(error: crate::bus::PublishError) -> Self {
        Self(format!("publish error: {error}"))
    }
}

impl From<crate::bus::ReceiveError> for TestError {
    fn from(error: crate::bus::ReceiveError) -> Self {
        Self(format!("receive error: {error}"))
    }
}

/// The test-result alias for kernel contract tests.
pub type Result<T> = core::result::Result<T, TestError>;
