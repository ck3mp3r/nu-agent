//! `AgentRuntimeBuilder::build` tests.
//!
//! `build` registers every tool group on a real rig `ToolServer` and creates the
//! audit log directory under the XDG data home. These tests drive the real
//! entry point and inspect the registered tool set.

use super::*;

use crate::bus::Bus;
use crate::conversation::builder::{AgentRuntimeBuilder, BuildArtifacts, BuildInput};
use crate::tools::closure::{ClosureParameter, ClosureRegistry, CwdEngine, ResolvedClosure};
use nu_protocol::engine::Closure;
use nu_protocol::{BlockId, Span, Spanned};
use std::time::Duration;

// region:    --- Test Support

/// Points `XDG_DATA_HOME` at a tempdir for the lifetime of the value.
///
/// `AgentRuntimeBuilder::build` creates the audit log directory under the XDG
/// data home, so the test must not write into the developer's real home.
struct DataHomeGuard {
    previous: Option<std::ffi::OsString>,
}

impl DataHomeGuard {
    fn apply(dir: &std::path::Path) -> Self {
        let previous = std::env::var_os("XDG_DATA_HOME");
        // SAFETY: every test that applies this guard is `#[serial]`, so no
        // other test reads the environment while it is set.
        unsafe { std::env::set_var("XDG_DATA_HOME", dir) };
        Self { previous }
    }
}

impl Drop for DataHomeGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            // SAFETY: see `DataHomeGuard::apply`.
            Some(previous) => unsafe { std::env::set_var("XDG_DATA_HOME", previous) },
            // SAFETY: see `DataHomeGuard::apply`.
            None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
        }
    }
}

/// A `ResolvedClosure` with one required parameter.
fn resolved_closure() -> ResolvedClosure {
    ResolvedClosure {
        closure: Spanned {
            item: Closure {
                block_id: BlockId::new(0),
                captures: Vec::new(),
            },
            span: Span::unknown(),
        },
        params: vec![ClosureParameter {
            name: "x".to_string(),
            is_required: true,
        }],
    }
}

/// The names registered on `handle`, in registration order.
fn registered_names(handle: &rig::tool::server::ToolServerHandle) -> Vec<String> {
    handle
        .static_tool_defs()
        .into_iter()
        .map(|def| def.name.as_str().to_string())
        .collect()
}

/// The names `builtin_tool_definitions` produces, in order.
fn builtin_names() -> Result<Vec<String>> {
    Ok(builtin_tool_definitions()?
        .into_iter()
        .map(|def| def.name.as_str().to_string())
        .collect())
}

/// Build a runtime with `registry` and `merged_compaction` over a tempdir.
async fn build_with(
    registry: &ClosureRegistry,
    merged_compaction: CompactionConfig,
) -> Result<(
    rig::tool::server::ToolServerHandle,
    BuildArtifacts,
    tempfile::TempDir,
)> {
    let temp = tempfile::tempdir()?;
    let guard = DataHomeGuard::apply(temp.path());
    let handle = rig::tool::server::ToolServer::new().run();
    let cwd = temp.path().to_path_buf();
    let artifacts = AgentRuntimeBuilder::new(BuildInput {
        tool_server_handle: &handle,
        closure_registry: registry,
        cwd: cwd.clone(),
        engine: CwdEngine::shared(cwd.to_string_lossy().to_string()),
        span: Span::unknown(),
        available_agents: &[],
        messaging_identity: None,
        tool_timeout: Duration::from_secs(5),
        session: None,
        max_tool_result_bytes: 20_000,
        bus: Bus::default(),
        merged_compaction,
    })
    .build()
    .await
    .map_err(|e| format!("build must succeed: {e}"))?;
    drop(guard);
    Ok((handle, artifacts, temp))
}

// endregion: --- Test Support

#[tokio::test]
#[serial_test::serial]
async fn build_with_empty_closure_registry_registers_only_builtins() -> Result<()> {
    // -- Setup & Fixtures
    let registry = ClosureRegistry::default();

    // -- Exec
    let (handle, artifacts, _temp) = build_with(&registry, CompactionConfig::default()).await?;

    // -- Check
    assert_eq!(
        registered_names(&handle),
        builtin_names()?,
        "an empty closure registry must register exactly the builtin tools"
    );
    assert_eq!(artifacts.parent_name, None);
    assert_eq!(
        artifacts.compaction_strategy,
        CompactionParams::default().compaction_strategy,
        "an unset compaction config must fall back to the default strategy"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn build_with_one_closure_registers_the_closure_tool() -> Result<()> {
    // -- Setup & Fixtures
    let mut registry = ClosureRegistry::default();
    registry.register("cov_closure".to_string(), resolved_closure());

    // -- Exec
    let (handle, _artifacts, _temp) = build_with(&registry, CompactionConfig::default()).await?;

    // -- Check
    let names = registered_names(&handle);
    assert!(
        names.contains(&"cov_closure".to_string()),
        "the closure tool must be registered on the tool server, got: {names:?}"
    );
    assert_eq!(
        names.len(),
        builtin_names()?.len() + 1,
        "exactly one closure tool must be added to the builtin set"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn build_applies_the_merged_compaction_strategy() -> Result<()> {
    // -- Setup & Fixtures
    let registry = ClosureRegistry::default();
    let merged = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.5),
    };

    // -- Exec
    let (_handle, artifacts, _temp) = build_with(&registry, merged).await?;

    // -- Check
    assert_eq!(
        artifacts.compaction_strategy,
        CompactionStrategy::SlidingSummary,
        "the merged strategy must reach the artifacts"
    );
    assert_eq!(
        artifacts.compaction_params.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
    assert_eq!(
        artifacts.merged_compaction.proactive_threshold_pct,
        Some(0.5)
    );
    Ok(())
}
