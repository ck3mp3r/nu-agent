//! `SessionState`, `SessionPersistence`, and accessor tests driven through the
//! real `AgentConversationRuntime`.

use super::harness::*;
use super::*;
use crate::session::management::{SessionPersistence, SessionState};
use agnt_runtime::session::prefix::dir_prefix;

/// Seed the rig-memory store of `id` with `messages`, then write the state
/// records so the session's `meta.json` listing carries the message count.
async fn seed_session(
    fixture: &RuntimeFixture,
    id: &str,
    messages: &[crate::types::Message],
) -> Result<()> {
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(id)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, id)?;
    conversation.extend(messages.iter().cloned());
    rig_memory::save(&*store, id, &mut conversation)?;
    agnt_runtime::session::rig_state::write_state_records(
        Arc::clone(&store),
        "test/model".to_string(),
        None,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn clear_session_resets_token_tracking_and_keeps_the_store() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    let session_id = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("fixture must carry a session id")?;
    seed_session(
        &fixture,
        &session_id,
        &[crate::types::Message::user("hello")],
    )
    .await?;
    fixture.runtime.session.last_total_tokens_mut().replace(99);

    // -- Exec
    fixture.runtime.clear_session();

    // -- Check
    assert_eq!(
        fixture.runtime.session.last_total_tokens(),
        None,
        "clearing a session must reset the token counter"
    );
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(&session_id)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let stored = rig_memory::load(&*store, &session_id)?;
    assert_eq!(
        stored.messages().len(),
        1,
        "clearing a session must not touch the append-only store"
    );
    Ok(())
}

#[tokio::test]
async fn new_session_derives_a_prefixed_id_from_the_cwd() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    let previous = fixture.runtime.final_session_id.clone();
    let expected_prefix = dir_prefix(&fixture.runtime.cwd);

    // -- Exec
    fixture.runtime.new_session();

    // -- Check
    let new_id = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("new_session must set a session id")?;
    assert!(
        new_id.starts_with(&format!("{expected_prefix}-")),
        "the new session id must carry the cwd prefix; got: {new_id}"
    );
    assert_ne!(Some(new_id), previous, "new_session must mint a fresh id");
    Ok(())
}

#[tokio::test]
async fn seed_last_total_tokens_sets_the_counter() {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused")).expect("fixture");

    // -- Exec
    fixture.runtime.seed_last_total_tokens(Some(4242));

    // -- Check
    assert_eq!(fixture.runtime.session.last_total_tokens(), Some(4242));
}

#[tokio::test]
async fn session_persistence_reports_cwd_and_current_session_id() {
    // -- Setup & Fixtures
    let fixture = make_runtime(text_model("unused")).expect("fixture");

    // -- Exec & Check
    assert_eq!(fixture.runtime.cwd(), fixture.root());
    assert_eq!(fixture.runtime.current_session_id(), Some("test-session"));
}

#[tokio::test]
async fn run_compaction_without_a_session_is_a_noop() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.runtime.final_session_id = None;

    // -- Exec
    fixture
        .runtime
        .run_compaction(CompactionSource::Manual)
        .await
        .map_err(|e| format!("compaction without a session must succeed: {e}"))?;

    // -- Check
    assert_eq!(fixture.runtime.current_session_id(), None);
    Ok(())
}

#[tokio::test]
async fn run_compaction_on_an_empty_session_is_a_noop() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;

    // -- Exec
    fixture
        .runtime
        .run_compaction(CompactionSource::Manual)
        .await
        .map_err(|e| format!("compaction of an empty session must succeed: {e}"))?;

    // -- Check
    assert_eq!(fixture.runtime.current_session_id(), Some("test-session"));
    Ok(())
}

#[tokio::test]
async fn run_compaction_summarizes_a_populated_session() -> Result<()> {
    // -- Setup & Fixtures
    // The conversation lives in the rig-memory store, which is what a run loads.
    let mut fixture = make_runtime(text_model("unused"))?;
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store("test-session")
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, "test-session")?;
    conversation.extend([
        crate::types::Message::user("first"),
        crate::types::Message::assistant("reply"),
        crate::types::Message::user("second"),
        crate::types::Message::assistant("second reply"),
    ]);
    rig_memory::save(&*store, "test-session", &mut conversation)?;

    // -- Exec
    fixture
        .runtime
        .run_compaction(CompactionSource::Manual)
        .await
        .map_err(|e| format!("compaction should succeed: {e}"))?;

    // -- Check
    // The log records the splice under the `rig.compaction` author.
    let stored = rig_memory::load(&*store, "test-session")?;
    let first = stored
        .entries()
        .next()
        .ok_or("the compacted conversation is non-empty")?;
    assert_eq!(
        first.author,
        Some(&rig_memory::summary_author()),
        "a successful compaction must splice the summary under rig.compaction"
    );
    Ok(())
}

#[tokio::test]
async fn load_session_hydrates_an_exact_match() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    seed_session(
        &fixture,
        "test-session",
        &[crate::types::Message::user("persisted prompt")],
    )
    .await?;

    // -- Exec
    let transcript = fixture
        .runtime
        .load_session("test-session")
        .await
        .map_err(|e| format!("load_session should succeed: {e}"))?;

    // -- Check
    assert_eq!(fixture.runtime.current_session_id(), Some("test-session"));
    assert_eq!(transcript.len(), 1, "the persisted message must hydrate");
    assert_eq!(transcript[0].content(), "persisted prompt");
    Ok(())
}

#[tokio::test]
async fn load_session_resolves_a_suffix_match() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    seed_session(
        &fixture,
        "prefix-abc123",
        &[crate::types::Message::user("suffix match")],
    )
    .await?;

    // -- Exec
    let transcript = fixture
        .runtime
        .load_session("abc123")
        .await
        .map_err(|e| format!("suffix load_session should succeed: {e}"))?;

    // -- Check
    assert_eq!(fixture.runtime.current_session_id(), Some("prefix-abc123"));
    assert_eq!(transcript.len(), 1);
    assert_eq!(transcript[0].content(), "suffix match");
    Ok(())
}

#[tokio::test]
async fn load_session_reports_an_unknown_id() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;

    // -- Exec
    let error = fixture
        .runtime
        .load_session("no-such-session")
        .await
        .err()
        .ok_or("an unknown session id must fail")?;

    // -- Check
    assert!(
        error.contains("not found"),
        "the error must report the missing session; got: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn list_sessions_filters_to_the_runtime_cwd() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = make_runtime(text_model("unused"))?;
    let prefix = dir_prefix(&fixture.runtime.cwd);
    seed_session(
        &fixture,
        &format!("{prefix}-mine"),
        &[crate::types::Message::user("mine")],
    )
    .await?;
    seed_session(
        &fixture,
        "other-cwd-session",
        &[crate::types::Message::user("other")],
    )
    .await?;

    // -- Exec
    let sessions = fixture
        .runtime
        .list_sessions(&fixture.runtime.cwd)
        .await
        .map_err(|e| format!("list_sessions should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        sessions.len(),
        1,
        "only sessions carrying the cwd prefix may be listed"
    );
    assert_eq!(sessions[0].id, format!("{prefix}-mine"));
    Ok(())
}

#[tokio::test]
async fn list_sessions_omits_sessions_without_messages() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = make_runtime(text_model("unused"))?;
    let prefix = dir_prefix(&fixture.runtime.cwd);
    seed_session(&fixture, &format!("{prefix}-empty"), &[]).await?;
    seed_session(
        &fixture,
        &format!("{prefix}-filled"),
        &[crate::types::Message::user("filled")],
    )
    .await?;

    // -- Exec
    let sessions = fixture
        .runtime
        .list_sessions(&fixture.runtime.cwd)
        .await
        .map_err(|e| format!("list_sessions should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        sessions.len(),
        1,
        "a session without messages must not be listed; got: {sessions:?}"
    );
    assert_eq!(sessions[0].id, format!("{prefix}-filled"));
    Ok(())
}

#[tokio::test]
async fn list_sessions_omits_the_current_session() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    let prefix = dir_prefix(&fixture.runtime.cwd);
    let current_id = format!("{prefix}-current");
    fixture.runtime.final_session_id = Some(current_id.clone());
    seed_session(
        &fixture,
        &current_id,
        &[crate::types::Message::user("current")],
    )
    .await?;
    seed_session(
        &fixture,
        &format!("{prefix}-other"),
        &[crate::types::Message::user("other")],
    )
    .await?;

    // -- Exec
    let sessions = fixture
        .runtime
        .list_sessions(&fixture.runtime.cwd)
        .await
        .map_err(|e| format!("list_sessions should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        sessions.len(),
        1,
        "the current session must not be listed; got: {sessions:?}"
    );
    assert_eq!(sessions[0].id, format!("{prefix}-other"));
    Ok(())
}

#[tokio::test]
async fn attach_session_hydrates_a_known_session() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    seed_session(
        &fixture,
        "attached-session",
        &[crate::types::Message::user("attached prompt")],
    )
    .await?;

    // -- Exec
    let transcript = fixture
        .runtime
        .attach_session("attached-session")
        .await
        .map_err(|e| format!("attach_session should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        fixture.runtime.current_session_id(),
        Some("attached-session")
    );
    assert_eq!(transcript.len(), 1);
    assert_eq!(transcript[0].content(), "attached prompt");
    Ok(())
}

#[tokio::test]
async fn attach_session_returns_an_empty_transcript_for_an_unknown_session() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;

    // -- Exec
    let transcript = fixture
        .runtime
        .attach_session("fresh-session")
        .await
        .map_err(|e| format!("attach_session should succeed: {e}"))?;

    // -- Check
    assert!(transcript.is_empty(), "a fresh session starts empty");
    assert_eq!(fixture.runtime.current_session_id(), Some("fresh-session"));
    Ok(())
}

#[tokio::test]
async fn accessors_report_the_constructed_runtime_state() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = make_runtime(text_model("unused"))?;

    // -- Exec
    let spawned = fixture.runtime.spawn(async { 7u8 }).await?;

    // -- Check
    assert_eq!(spawned, 7);
    assert_eq!(fixture.runtime.provider_name(), "mock");
    assert_eq!(fixture.runtime.model(), "mock-model");
    assert!(fixture.runtime.startup_plugin_config().is_none());
    assert_eq!(fixture.runtime.agent_identity(), Some("test-agent"));
    assert_eq!(
        fixture.runtime.persona_body_len(),
        Some("persona body".len())
    );
    assert_eq!(fixture.runtime.mcp_caller_cwd(), Some(fixture.root()));
    assert!(fixture.runtime.mcp_lifecycle_projection().is_empty());
    assert!(fixture.runtime.available_agent_summaries().is_empty());
    assert!(fixture.runtime.interactive_pending_arc().is_none());
    Ok(())
}

#[tokio::test]
async fn interactive_pending_arc_clones_the_shared_map() {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused")).expect("fixture");
    let pending: PendingPermissions =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    fixture.runtime.interactive_pending = Some(std::sync::Arc::clone(&pending));

    // -- Exec
    let cloned = fixture.runtime.interactive_pending_arc();

    // -- Check
    let cloned = cloned.expect("interactive mode must expose the pending map");
    assert!(
        std::sync::Arc::ptr_eq(&cloned, &pending),
        "the accessor must clone the same shared map"
    );
}

/// WHEN the session changes, THE hook's threshold input SHALL NOT describe the
/// session just left: neither the last real token count nor a store-dead latch
/// survives the switch.
#[tokio::test]
async fn session_switch_drops_the_previous_sessions_threshold_inputs() -> Result<()> {
    // -- Setup & Fixtures: a total and a latch recorded under another session.
    let mut fixture = make_runtime(text_model("unused"))?;
    *fixture
        .runtime
        .last_total_tokens
        .lock()
        .map_err(|_| "token slot poisoned")? = Some(500_000);
    *fixture
        .runtime
        .store_latch
        .lock()
        .map_err(|_| "store latch poisoned")? = Some("dead".to_string());

    // -- Exec
    fixture.runtime.clear_session();

    // -- Check: a conversation under the threshold can no longer fire a
    // decision from the previous session's total.
    assert_eq!(
        *fixture
            .runtime
            .last_total_tokens
            .lock()
            .map_err(|_| "token slot poisoned")?,
        None,
        "the previous session's total must not survive the switch"
    );
    assert!(
        fixture
            .runtime
            .store_latch
            .lock()
            .map_err(|_| "store latch poisoned")?
            .is_none(),
        "the previous session's store latch must not survive the switch"
    );
    Ok(())
}

/// WHEN the runtime attaches to another session, THE hook's threshold input
/// SHALL NOT describe the session just left — the attach is a switch too.
#[tokio::test]
async fn attach_session_drops_the_previous_sessions_threshold_inputs() -> Result<()> {
    // -- Setup & Fixtures: a total and a latch recorded under another session.
    let mut fixture = make_runtime(text_model("unused"))?;
    let previous = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("fixture must start attached to a session")?;
    *fixture
        .runtime
        .last_total_tokens
        .lock()
        .map_err(|_| "token slot poisoned")? = Some(500_000);
    *fixture
        .runtime
        .store_latch
        .lock()
        .map_err(|_| "store latch poisoned")? = Some("dead".to_string());

    // -- Exec
    fixture.runtime.attach_session("attached-session").await?;

    // -- Check
    assert_ne!(
        fixture.runtime.final_session_id.as_deref(),
        Some(previous.as_str()),
        "the attach must move the runtime to the new session"
    );
    assert_eq!(
        *fixture
            .runtime
            .last_total_tokens
            .lock()
            .map_err(|_| "token slot poisoned")?,
        None,
        "the previous session's total must not survive the attach"
    );
    assert!(
        fixture
            .runtime
            .store_latch
            .lock()
            .map_err(|_| "store latch poisoned")?
            .is_none(),
        "the previous session's store latch must not survive the attach"
    );
    Ok(())
}

/// WHEN the runtime attaches to a session whose turns settled, THE user row
/// SHALL carry the recorded facts: the attach joins by key exactly as the
/// startup resolver does.
#[tokio::test]
async fn attach_session_stamps_the_joined_turns_facts() -> Result<()> {
    // -- Setup & Fixtures: a store on disk, so the facts one open writes are the
    // facts the next open reads.
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.runtime.session = crate::conversation::state::memory::MemoryState::new(
        agnt_runtime::session::RigStoreConfig::new(agnt_runtime::session::StoreType::Sqlite)
            .with_cache_dir(fixture.root().join("attach-facts")),
    );
    let session_id = "test-attach-facts";
    seed_session(
        &fixture,
        session_id,
        &[crate::types::Message::user("the prompt")],
    )
    .await?;
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(session_id)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let saved = rig_memory::load(&*store, session_id)?;
    let keys = agnt_runtime::session::settle::entry_keys(saved.entries());
    let facts = fixture
        .runtime
        .session
        .inner_memory()
        .facts()
        .await
        .map_err(|e| format!("facts should open: {e:?}"))?;
    facts
        .settle(agnt_runtime::session::settle::SettleInput {
            session_id,
            model: "openai/gpt-5",
            agent: "coder",
            submitted_millis: 1_790_974_800_000,
            keys: Some(keys),
            records: &[],
        })
        .await?;

    // -- Exec
    let snapshots = fixture.runtime.attach_session(session_id).await?;

    // -- Check: the attach path's row carries the recorded facts.
    let user = snapshots
        .iter()
        .find(|snapshot| snapshot.role() == "user")
        .ok_or("the attach must hydrate the prompt")?;
    assert_eq!(
        user.stamp_millis(),
        Some(1_790_974_800_000),
        "the attach joins by key exactly as the startup path does"
    );
    assert_eq!(user.stamp_model(), Some("openai/gpt-5"));
    assert_eq!(user.stamp_agent(), Some("coder"));
    Ok(())
}
