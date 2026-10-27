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

impl agnt_core::hooks::HookProcess for RecordingProcess {
    async fn run(
        &self,
        _script: &std::path::Path,
        event: &str,
        envelope: &str,
        _cwd: &std::path::Path,
        _timeout: std::time::Duration,
    ) -> core::result::Result<(), agnt_core::hooks::HookProcessError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((event.to_string(), envelope.to_string()));
        }
        Ok(())
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
pub(super) fn agent_context() -> agnt_core::hooks::HookAgentContext {
    agnt_core::hooks::HookAgentContext::default()
        .with_persona(Some("reviewer".to_string()))
        .with_model("openai/gpt-4o".to_string())
}
