//! The model switch's latency contract: a switch builds nothing.
//!
//! A switch swaps the model config and marks the shared model stale. The
//! provider client, and the model erased from it, build at the model's next use.
//! Both halves are visible here: a build future that never resolves cannot hold
//! the switch, and the switch leaves no client behind.
//!
//! Both tests fail on an eager switch — the first because it would await the
//! build, the second because the build would leave a client in the cache.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::harness::*;
use super::switch_support::*;
use super::*;
use crate::config::{Config, PluginConfig};
use crate::conversation::managers::ProviderManager;
use crate::conversation::providers::CachedProviderClient;
use crate::conversation::state::provider::ProviderState;
use crate::protocol::model_switching::ModelSwitching;
use nu_protocol::LabeledError;
use rig::DynModel;
use rig::operation::Completion;

/// A provider whose model build the test controls.
///
/// Every other answer is the real provider's, delegated: only the build — the
/// step that erases a model out of a client — is stood in for, so a switch that
/// awaited it would either hang or leave a client behind.
struct TestBuildProvider {
    inner: ProviderState,
    builds: Arc<AtomicUsize>,
    /// What a build answers. `None` never resolves, so a caller that awaits it
    /// cannot return.
    answer: Option<rig::test_utils::MockCompletionModel>,
}

impl TestBuildProvider {
    /// A provider whose build never resolves.
    fn unresolved(plugin_config: PluginConfig) -> Self {
        Self {
            inner: ProviderState::new(runtime_config(), Some(plugin_config)),
            builds: Arc::new(AtomicUsize::new(0)),
            answer: None,
        }
    }

    /// A provider whose build answers `answer` at once.
    fn ready(plugin_config: PluginConfig, answer: rig::test_utils::MockCompletionModel) -> Self {
        Self {
            inner: ProviderState::new(runtime_config(), Some(plugin_config)),
            builds: Arc::new(AtomicUsize::new(0)),
            answer: Some(answer),
        }
    }
}

impl ProviderManager for TestBuildProvider {
    fn provider_config(&self) -> &Config {
        self.inner.provider_config()
    }

    fn provider_config_mut(&mut self) -> &mut Config {
        self.inner.provider_config_mut()
    }

    async fn ensure_client_cached(&mut self) -> core::result::Result<(), LabeledError> {
        self.inner.ensure_client_cached().await
    }

    fn cached_client(&self) -> Option<&CachedProviderClient> {
        self.inner.cached_client()
    }

    fn invalidate_cache(&mut self) {
        self.inner.invalidate_cache()
    }

    fn switch_model(&mut self, model_spec: &str) -> core::result::Result<String, String> {
        self.inner.switch_model(model_spec)
    }

    async fn build_dyn_model(
        &mut self,
    ) -> core::result::Result<DynModel<Completion>, LabeledError> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        match &self.answer {
            Some(model) => Ok(model.clone().erase()),
            None => {
                std::future::pending::<()>().await;
                Err(LabeledError::new("the build future must stay pending"))
            }
        }
    }

    fn startup_plugin_config(&self) -> Option<&PluginConfig> {
        self.inner.startup_plugin_config()
    }
}

/// WHEN a switch runs while the model build never resolves, THEN the switch SHALL
/// return without awaiting that build.
#[tokio::test]
async fn switch_model_returns_while_the_model_build_stays_pending() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::tempdir()?;
    let provider = TestBuildProvider::unresolved(two_limit_plugin_config());
    let builds = Arc::clone(&provider.builds);
    let mut runtime = assemble_for_switch(provider, text_model("unused"), &temp_dir)?;

    // -- Exec
    let switched = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        runtime.switch_model("ollama/small"),
    )
    .await;

    // -- Check
    let (identity, _) = switched
        .map_err(|_| "the switch must not await the model build")?
        .map_err(|e| format!("the switch should succeed: {e}"))?;
    assert_eq!(identity, "ollama/small");
    assert_eq!(
        builds.load(Ordering::SeqCst),
        0,
        "the switch must attempt no model build"
    );
    assert!(
        runtime.dyn_model.is_pending(),
        "the switch leaves the model to build at its next use"
    );
    Ok(())
}

/// WHEN a switch resolves a provider whose client is not cached, THEN the switch
/// SHALL leave that client unbuilt.
#[tokio::test]
async fn switch_model_constructs_no_provider_client() -> Result<()> {
    // -- Setup & Fixtures: no cached client, so any build would create one.
    let temp_dir = tempfile::tempdir()?;
    let provider = ProviderState::new(runtime_config(), Some(two_limit_plugin_config()));
    let mut runtime = assemble_for_switch(provider, text_model("unused"), &temp_dir)?;

    // -- Exec
    let (identity, _) = runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("the switch should succeed: {e}"))?;

    // -- Check
    assert_eq!(identity, "ollama/small");
    assert!(
        runtime.provider.client().is_none(),
        "the switch must construct no provider client"
    );
    Ok(())
}

/// WHEN the model's next use follows a switch, THEN that use SHALL build the
/// model once and re-resolve the planning spec for the model it landed on.
#[tokio::test]
async fn ensure_active_model_builds_the_pending_model_once() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::tempdir()?;
    let provider = TestBuildProvider::ready(two_limit_plugin_config(), text_model("built"));
    let builds = Arc::clone(&provider.builds);
    let mut runtime = assemble_for_switch(provider, text_model("unused"), &temp_dir)?;
    let before_spec = runtime.compaction.spec();

    // -- Exec
    runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("the switch should succeed: {e}"))?;
    runtime
        .ensure_active_model()
        .await
        .map_err(|e| format!("the next use should build the model: {e}"))?;

    // -- Check
    assert_eq!(
        builds.load(Ordering::SeqCst),
        1,
        "the next use builds the model exactly once"
    );
    assert!(
        !runtime.dyn_model.is_pending(),
        "the build clears the staleness the switch set"
    );
    assert_eq!(
        runtime
            .compaction
            .spec()
            .and_then(|spec| spec.context_window),
        Some(1_000),
        "the planning spec follows the model the switch landed on"
    );
    assert_ne!(
        before_spec.and_then(|spec| spec.context_window),
        Some(1_000),
        "the fixture must start on another window, or the spec check proves nothing"
    );
    Ok(())
}
