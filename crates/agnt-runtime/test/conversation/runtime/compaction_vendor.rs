//! The planning spec follows the provider the wire reports.
//!
//! A session whose config spells its provider in a way rig does not register
//! must still compact. The runtime re-resolves the spec from the wire it built,
//! and the Copilot spellings resolve even before that build lands, so neither
//! the switch path nor either sweep can lose the session's compaction to a name
//! mismatch.

use super::harness::*;
use super::*;
use crate::config::Config;
use crate::conversation::providers::CachedProviderClient;
use crate::conversation::state::provider::ProviderState;
use crate::session::management::SessionPersistence;
use crate::tools::handler::McpToolRegistry;
use crate::types::Message;
use agnt_kernel::{CompactionRecord, CompactionSource, SessionRecord};
use rig::providers::registry::{Format, ProviderId};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

/// The session every flow test compacts.
const SESSION: &str = "test-session";

/// A config that spells its provider `github-copilot`, as a
/// `[providers.github-copilot]` block that declares no rig impl does.
fn copilot_config() -> Config {
    Config {
        provider: "github-copilot".to_string(),
        model: "gpt-4o".to_string(),
        max_retries: Some(0),
        ..Config::default()
    }
}

/// The id rig registers for the Copilot wire name.
fn wire_copilot_id() -> Result<ProviderId> {
    let name = rig::providers::copilot::DIALECT.name;
    ProviderId::new(name, Format::OpenAi)
        .ok_or_else(|| format!("rig must register its own Copilot wire name '{name}'").into())
}

/// A model that answers every summarizer call with text.
///
/// The model is the runtime's, and [`AgentRuntime::ensure_active_model`] hands
/// the rebuilt model to the compactor, so it must answer the split turn's two
/// calls as well as the history's.
fn summary_model() -> MockCompletionModel {
    MockCompletionModel::from_stream_turns((0..4).map(|_| {
        vec![
            MockStreamEvent::Text("summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ]
    }))
}

/// A runtime whose provider spells Copilot `github-copilot`.
///
/// The client is a mock, so no transport, TLS provider, or credential store is
/// touched: the Copilot spelling is the only thing under test.
fn copilot_fixture() -> Result<RuntimeFixture> {
    let model = summary_model();
    let provider =
        ProviderState::with_client(copilot_config(), CachedProviderClient::Mock(model.clone()));
    let temp_dir = tempfile::tempdir()?;
    let runtime = assemble(
        provider,
        model,
        &temp_dir,
        true,
        McpToolRegistry::empty(),
        Vec::new(),
    )?;
    Ok(RuntimeFixture { runtime, temp_dir })
}

/// Build the pending model, which re-resolves the spec from the wire it built.
async fn rebuild(fixture: &mut RuntimeFixture) -> Result<()> {
    fixture.runtime.dyn_model.mark_pending();
    fixture.runtime.ensure_active_model().await?;
    Ok(())
}

/// Seed the session's rig store with a conversation the summary replaces.
async fn seed(fixture: &RuntimeFixture) -> Result<()> {
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(SESSION)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, SESSION)?;
    conversation.extend([
        Message::user("first"),
        Message::assistant("reply"),
        Message::user("second"),
        Message::assistant("second reply"),
    ]);
    rig_memory::save(&*store, SESSION, &mut conversation)?;
    Ok(())
}

/// The compaction rows the stream carried, in publication order.
fn compaction_rows(
    rx: &mut agnt_kernel::bus::Subscription<SessionRecord>,
) -> Vec<CompactionRecord> {
    let mut rows = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Compaction(row) = record {
            rows.push(row);
        }
    }
    rows
}

/// WHEN the config spells the provider `github-copilot`, THE runtime's spec
/// re-resolution SHALL land a planning spec instead of disabling compaction.
#[tokio::test]
async fn test_runtime_compaction_vendor_resolves_the_copilot_spelling() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = copilot_fixture()?;

    // -- Exec: the next use rebuilds the wire and re-resolves the spec.
    rebuild(&mut fixture).await?;

    // -- Check
    let spec = fixture
        .runtime
        .compaction
        .spec()
        .ok_or("a Copilot session must not disable compaction")?;
    assert_eq!(
        spec.provider,
        wire_copilot_id()?,
        "the spec must be planned under the id rig registers for Copilot"
    );
    Ok(())
}

/// WHEN a sweep runs on a Copilot session, THEN it SHALL reach the splice
/// instead of refusing with `resolve-to-no-rig-provider-id`.
async fn assert_sweep_splices(source: CompactionSource) -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = copilot_fixture()?;
    seed(&fixture).await?;
    rebuild(&mut fixture).await?;
    let mut rows = fixture.runtime.bus.stream().subscribe();

    // -- Exec
    fixture
        .runtime
        .run_compaction(source)
        .await
        .map_err(|e| format!("a Copilot session's sweep must not fail: {e}"))?;

    // -- Check: the sweep started, and no row names a disabled compaction.
    let published = compaction_rows(&mut rows);
    assert!(
        published.iter().any(
            |row| matches!(row, CompactionRecord::Started { source: started } if *started == source)
        ),
        "a {source:?} sweep must reach the compaction ladder, got {published:?}"
    );
    assert!(
        !published
            .iter()
            .any(|row| matches!(row, CompactionRecord::Failed { .. })),
        "no sweep may report a failure on a Copilot session, got {published:?}"
    );

    // -- Check: the splice is what the store now holds.
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(SESSION)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let stored = rig_memory::load(&*store, SESSION)?;
    assert_eq!(
        stored.messages().len(),
        2,
        "the sweep must splice the summary in, leaving the summary and the newest message"
    );
    Ok(())
}

/// WHEN the manual `/compact` flow runs on a Copilot session, THEN it SHALL
/// reach the splice.
#[tokio::test]
async fn test_runtime_compaction_vendor_manual_sweep_splices() -> Result<()> {
    // -- Exec & Check
    assert_sweep_splices(CompactionSource::Manual).await
}

/// WHEN the auto-threshold flow runs on a Copilot session, THEN it SHALL reach
/// the splice.
#[tokio::test]
async fn test_runtime_compaction_vendor_auto_sweep_splices() -> Result<()> {
    // -- Exec & Check
    assert_sweep_splices(CompactionSource::Auto).await
}
