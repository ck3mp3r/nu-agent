//! Shared test doubles for the CLI mode tests.

use super::*;

/// Records every hook invocation and always succeeds.
#[derive(Clone, Default)]
pub(super) struct RecordingProcess {
    calls: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

impl RecordingProcess {
    pub(super) fn calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl agnt_runtime::hooks::HookProcess for RecordingProcess {
    async fn run(
        &self,
        _script: &std::path::Path,
        event: &str,
        envelope: &str,
        _cwd: &std::path::Path,
        _timeout: std::time::Duration,
    ) -> core::result::Result<(), agnt_runtime::hooks::HookProcessError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((event.to_string(), envelope.to_string()));
        }
        Ok(())
    }
}

/// Records every hook invocation after `delay`, and always succeeds.
///
/// Stands in for a `nu` subprocess whose spawn and startup cost grows under
/// load: the runner's invocations are serialized, so a pair of them takes twice
/// the delay.
#[derive(Clone)]
pub(super) struct DelayedProcess {
    calls: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
    delay: std::time::Duration,
}

impl DelayedProcess {
    pub(super) fn new(delay: std::time::Duration) -> Self {
        Self {
            calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            delay,
        }
    }

    pub(super) fn calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl agnt_runtime::hooks::HookProcess for DelayedProcess {
    async fn run(
        &self,
        _script: &std::path::Path,
        event: &str,
        envelope: &str,
        _cwd: &std::path::Path,
        _timeout: std::time::Duration,
    ) -> core::result::Result<(), agnt_runtime::hooks::HookProcessError> {
        tokio::time::sleep(self.delay).await;
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((event.to_string(), envelope.to_string()));
        }
        Ok(())
    }
}

/// Never finishes a hook invocation: the runner is still inside it at exit.
#[derive(Clone, Default)]
pub(super) struct HangingProcess;

impl agnt_runtime::hooks::HookProcess for HangingProcess {
    async fn run(
        &self,
        _script: &std::path::Path,
        _event: &str,
        _envelope: &str,
        _cwd: &std::path::Path,
        _timeout: std::time::Duration,
    ) -> core::result::Result<(), agnt_runtime::hooks::HookProcessError> {
        std::future::pending().await
    }
}

/// A resolved hook subscribing to `events`, with a script path that is never run.
pub(super) fn resolved_hook(name: &str, events: &[HookEvent]) -> ResolvedHook {
    ResolvedHook {
        name: name.to_string(),
        events: events.to_vec(),
        script: std::path::PathBuf::from(format!("/hooks/{name}.nu")),
        timeout: std::time::Duration::from_secs(10),
    }
}

/// The agent snapshot every hook envelope is stamped with.
pub(super) fn agent_context() -> agnt_runtime::hooks::HookAgentContext {
    agnt_runtime::hooks::HookAgentContext::default()
        .with_persona(Some("reviewer".to_string()))
        .with_model("openai/gpt-4o".to_string())
}
