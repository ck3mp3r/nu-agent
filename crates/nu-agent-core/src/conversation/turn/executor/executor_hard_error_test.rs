//! Hard-error persistence tests for the turn executor.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::config::Config;
use crate::session::StoreEntry;

// ---------------------------------------------------------------------------
// Error path persistence tests
// ---------------------------------------------------------------------------

/// MaxTurnsError carries full chat_history — executor must persist it and return Err.
///
/// Setup: mock returns a tool_call on turn 1. Config limits tool turns to 0, so rig
/// raises MaxTurnsError after the first tool call attempt. After Fix 1, TurnError
/// gets messages=Some(chat_history). After Fix 2, executor persists those messages
/// before returning LabeledError.
#[tokio::test]
async fn max_turns_error_persists_full_history() -> Result<()> {
    // max_tool_turns=0: rig raises MaxTurnsError as soon as a tool-call turn would
    // be scheduled (current_turn > 0 + 1 after the first tool response).
    let config = Config {
        max_tool_turns: Some(0),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-max-turns";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: model asks for a tool call. With max_turns=0, rig will MaxTurnsError
    // as soon as it tries to schedule the tool-call turn.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tool_call_1", "some_tool", serde_json::json!({"x": 1})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "please call a tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err (it's a hard error, not a cancellation)
    assert!(
        result.is_err(),
        "MaxTurnsError must propagate as LabeledError to caller"
    );

    // JSONL must have been written with the partial chat history
    let persisted = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !persisted.is_empty(),
        "MaxTurnsError must persist chat_history to JSONL (Fix 1 + Fix 2 path A history)"
    );

    Ok(())
}

/// UnknownToolCall carries full chat_history — executor must persist it and return Err.
///
/// Setup: mock returns a tool_call for "nonexistent_tool" which is not registered
/// in the agent's tool list. Rig raises UnknownToolCall. After Fix 1, TurnError
/// gets messages=Some(chat_history). After Fix 2, executor persists them.
#[tokio::test]
async fn unknown_tool_error_persists_full_history() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-unknown-tool";
    let mut memory_state = make_memory_state(&temp_dir);

    // Model calls a tool that is not registered — triggers UnknownToolCall.
    // No visible_tool_definitions → agent has no tools → any tool call is unknown.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "tool_call_1",
            "nonexistent_tool",
            serde_json::json!({"arg": "value"}),
        ),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "use a tool please".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "UnknownToolCall must propagate as LabeledError to caller"
    );

    // JSONL must have been written with the partial chat history
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !persisted.is_empty(),
        "UnknownToolCall must persist chat_history to JSONL (Fix 1 + Fix 2 path A history)"
    );

    Ok(())
}

/// Network/CompletionError on a fresh session — executor persists the user prompt
/// via the delta path (last_known_history = [user_prompt] after fix).
///
/// After the `on_completion_call` fix, `last_known_history` = `history + [prompt]` =
/// `[] + [user_msg]` = `[user_msg]`. delta = skip(0) = `[user_msg]` (non-empty),
/// so the delta path fires and persists just the user message. The placeholder path
/// is no longer triggered for this case.
#[tokio::test]
async fn network_error_on_fresh_session_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-network-error";
    let prompt_text = "what is the weather today?";
    let mut memory_state = make_memory_state(&temp_dir);

    // Streaming error on the first event — simulates network failure.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "network error must propagate as LabeledError to caller"
    );

    // After the fix: last_known_history = [user_msg], delta = skip(0) = [user_msg].
    // Delta path fires → 1 message persisted (just the user prompt).
    // The placeholder path no longer fires because the delta is non-empty.
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        persisted.len(),
        1,
        "hard error on fresh session must persist exactly 1 message (user prompt via delta path); got {} messages",
        persisted.len()
    );
    // The single persisted message must be the user prompt
    assert!(
        matches!(persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be a User message"
    );

    Ok(())
}

/// When `on_completion_call` fires before a CompletionError on a fresh session,
/// the executor persists the user prompt via the delta path (not a placeholder pair).
///
/// After the fix, `on_completion_call(prompt, history=[]`) stores `[] + [user_prompt]`
/// = `[user_prompt]`. `pre_turn_message_count = 0`, so `delta = skip(0) = [user_prompt]`
/// (non-empty) → delta path fires → 1 message persisted (just the user prompt).
///
/// The placeholder path (which would produce 2 messages) is no longer triggered
/// because the delta is now always non-empty when `on_completion_call` has fired.
///
/// This is CORRECT: we now save the user's question even if the API fails to respond,
/// which is better than saving a fake `[Turn failed:]` assistant message.
#[tokio::test]
async fn hard_error_on_first_llm_call_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-no-hook-history";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state = make_memory_state(&temp_dir);

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] → delta path fires → 1 message persisted.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "hard error must propagate as Err to caller"
    );

    // After the fix: last_known_history = [user_msg], delta = [user_msg], 1 message persisted.
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(
        persisted.len(),
        1,
        "hard error on fresh session must persist exactly 1 message (user prompt); got {:?}",
        persisted
    );

    // The single persisted message must be the user prompt (not a placeholder)
    assert!(
        matches!(persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be a User message; got {:?}",
        persisted[0]
    );

    Ok(())
}

/// When there is no session (transient invocation), hard errors must NOT write
/// anything to the store — there is no conversation to record.
#[tokio::test]
async fn hard_error_no_session_persists_nothing() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);

    // Streaming error — hard failure, no history recoverable.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "a transient prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            None, // <-- no session
        )
        .await;

    // Must return Err
    assert!(result.is_err(), "hard error must propagate as LabeledError");

    // No JSONL should have been written — transient invocation with no session_id
    // There is no specific conversation_id to check, so we verify the temp dir
    // has no .jsonl files written (the store uses conversation_id as filename).
    let jsonl_files: Vec<_> = std::fs::read_dir(temp_dir.path())
        .expect("read_dir should succeed")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    assert!(
        jsonl_files.is_empty(),
        "no JSONL files must be written for a transient (no-session) hard error; found: {:?}",
        jsonl_files.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// CompletionError + hook history recovery tests
// ---------------------------------------------------------------------------

/// Hard error after prior session history: the user prompt is persisted via the delta
/// path (not a placeholder pair), because `last_known_history` now includes the prompt.
///
/// **Why the delta is [user_prompt] after the fix:** `on_completion_call(prompt, history)`
/// now stores `history + [prompt]`. With prior history = [prior_1, prior_2] and a new
/// user prompt, `last_known_history` = [prior_1, prior_2, user_prompt].
/// `skip(pre_turn_message_count=2)` = [user_prompt] → delta path fires → 1 new message.
///
/// Before the fix: `last_known_history` = [prior_1, prior_2] (no prompt included).
/// `skip(2)` = empty delta → placeholder pair fired → 4 messages (2 prior + 2 placeholder).
///
/// After the fix: delta = [user_prompt] → delta path fires → 3 messages total
/// (2 prior + 1 user_prompt). The placeholder path no longer fires.
///
/// Key regression assertion: store must be exactly 3 (2 prior + 1 user prompt),
/// NOT 4 (old placeholder pair) and NOT 5 (the doubled result from the pre-delta-fix bug).
#[tokio::test]
async fn hard_error_after_prior_history_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-hook-history";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate the session store with a completed exchange so that rig
    // loads it into the agent's context and on_completion_call fires with
    // non-empty prior history.
    let prior_messages = vec![
        crate::types::Message::user("work done"),
        crate::types::Message::assistant("ok"),
    ];
    // Pre-populate the store by creating a separate FsSessionStore pointing to the same path
    // (the memory_state's internal store shares the same backing directory)
    let _prior_entries: Vec<StoreEntry> = prior_messages
        .iter()
        .cloned()
        .map(StoreEntry::Message)
        .collect();
    memory_state.inner_memory().load_all(session_id).await.ok();
    // Use ConversationMemory append to pre-populate
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, prior_messages.clone())
            .await
            .map_err(|e| format!("append prior messages: {e:?}"))?;
    }

    // Mock model: errors immediately (simulates CompletionError / network failure).
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("http decode error")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err (it's a hard error)
    assert!(result.is_err(), "CompletionError must propagate as Err");

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // After the fix: last_known_history = [prior_1, prior_2, user_prompt].
    // delta = skip(2) = [user_prompt] → delta path fires → 3 messages total.
    // NOT 4 (old placeholder pair), NOT 5 (doubled history from pre-delta bug).
    assert_eq!(
        persisted.len(),
        3,
        "hard error after prior history must persist exactly 3 messages \
         (2 prior + 1 user prompt via delta path); got {:?}",
        persisted
    );

    // [0] must be the prior user message
    assert!(
        matches!(&persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be User (prior); got {:?}",
        persisted[0]
    );
    // [1] must be the prior assistant message
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant { .. }),
        "persisted[1] must be Assistant (prior); got {:?}",
        persisted[1]
    );
    // [2] must be the user prompt ("new prompt")
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "persisted[2] must be User (new prompt); got {:?}",
        persisted[2]
    );
    // Verify [2] contains the new prompt text
    let new_prompt_text = match &persisted[2] {
        crate::types::Message::User { content } => content
            .iter()
            .find_map(|c| {
                if let crate::types::UserContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        other => panic!("persisted[2] must be User; got {:?}", other),
    };
    assert_eq!(
        new_prompt_text, "new prompt",
        "persisted[2] must contain the new user prompt text"
    );

    // Prior message content check
    let has_prior_user = persisted.iter().any(|msg| {
        if let crate::types::Message::User { content } = msg {
            content.iter().any(|item| {
                if let crate::types::UserContent::Text(t) = item {
                    t.text == "work done"
                } else {
                    false
                }
            })
        } else {
            false
        }
    });
    assert!(
        has_prior_user,
        "prior user message must still be in store; got messages: {:?}",
        persisted
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Subtask 2 — delta-only persistence regression tests
// ---------------------------------------------------------------------------

/// Regression test: hard error after prior session history must not re-append the
/// full hook history snapshot — only a delta (the user prompt from this turn).
///
/// Before the fix: `last_known_history` (full history) was appended → store grew
/// from 2 to 5 messages (2 prior + 3 full history re-appended).
/// After the fix: `last_known_history` = [prior_1, prior_2, user_prompt].
/// delta = skip(pre_turn_count=2) = [user_prompt] → delta path fires → 3 messages total.
///
/// The bound is now exactly 3 (not < 5). The delta path fires with just the user
/// prompt — no placeholder is synthesised because the delta is non-empty.
#[tokio::test]
async fn hard_error_after_prior_history_persists_only_delta() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-delta-hard-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate: simulate a prior successful turn using ConversationMemory append
    let prior_msgs = vec![
        crate::types::Message::user("prior work"),
        crate::types::Message::assistant("done"),
    ];
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, prior_msgs)
            .await
            .map_err(|e| format!("append prior messages: {e:?}"))?;
    }

    // Model errors immediately — simulates CompletionError / network failure.
    // on_completion_call fires with history = [user("prior work"), assistant("done")]
    // and prompt = user("new question").
    // After fix: last_known_history = [prior_1, prior_2, user_prompt].
    // delta = skip(2) = [user_prompt] → delta path fires → 3 total.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new question".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_err(), "hard error must propagate as Err");

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // After the fix: exactly 3 messages (2 prior + 1 user prompt delta).
    // The delta path fires because last_known_history includes the user prompt.
    // NOT 5 (pre-delta-fix duplication bug), NOT 4 (old placeholder pair).
    assert_eq!(
        persisted.len(),
        3,
        "hard error after prior history must persist exactly 3 messages (2 prior + 1 user prompt delta); got {} messages",
        persisted.len()
    );
    assert!(
        persisted.len() >= 2,
        "prior messages must be preserved; got {}",
        persisted.len()
    );

    Ok(())
}

/// Regression test: two consecutive hard errors must not double history each time.
///
/// Before the fix: turn 2 error appended full history (3 msgs), turn 3 appended full
/// history again (4 msgs) → store grew to 2 + 3 + 4 = 9 messages.
/// After the fix: `on_completion_call` stores `history + [prompt]`, so for each error
/// turn the delta = [user_prompt_for_that_turn] (1 message). The delta path fires.
/// Store grows by 1 per error turn:
///   - Turn 1 success: 2 messages (rig persists [user("t1"), assistant("ok")])
///   - Turn 2 error: +1 = 3 messages (delta = [user("t2")])
///   - Turn 3 error: +1 = 4 messages (delta = [user("t3")])
#[tokio::test]
async fn hard_error_twice_does_not_double_history() -> Result<()> {
    let config = test_config();
    let session_id = "test-no-double";
    let temp_dir = tempfile::tempdir().unwrap();

    // Turn 1: successful turn — rig appends [user("t1"), assistant("ok")] → store has 2 msgs.
    {
        let mut memory_state = make_memory_state(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("ok".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let mut executor = make_executor(
            &config,
            &mut memory_state,
            shared_model,
            default_tool_infra(crate::bus::create_bus()),
        );
        let result = executor
            .execute(
                ExecuteInput {
                    prompt: "t1".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        assert!(result.is_ok(), "turn 1 must succeed");
    }

    // Turn 2: hard error. pre_turn_count=2. last_known_history = [prior_1, prior_2, user("t2")].
    // delta = skip(2) = [user("t2")] → delta path fires → store has 3.
    {
        let mut memory_state = make_memory_state(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let mut executor = make_executor(
            &config,
            &mut memory_state,
            shared_model,
            default_tool_infra(crate::bus::create_bus()),
        );
        let _ = executor
            .execute(
                ExecuteInput {
                    prompt: "t2".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
    }

    // Turn 3: hard error again. pre_turn_count=3. last_known_history = [prior_1, prior_2, user("t2"), user("t3")].
    // delta = skip(3) = [user("t3")] → delta path fires → store has 4.
    {
        let mut memory_state = make_memory_state(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let mut executor = make_executor(
            &config,
            &mut memory_state,
            shared_model,
            default_tool_infra(crate::bus::create_bus()),
        );
        let _ = executor
            .execute(
                ExecuteInput {
                    prompt: "t3".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
    }

    // Final state: 2 (turn 1 success) + 1 (turn 2 user delta) + 1 (turn 3 user delta) = 4.
    // After the fix: on_completion_call stores history + [prompt], so delta = [user_prompt]
    // for each error turn. Delta path fires → 1 message per error turn, not 2 (no placeholder).
    let final_memory_state = make_memory_state(&temp_dir);
    let final_entries = final_memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let final_count: Vec<crate::types::Message> = final_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    let final_count = final_count.len();

    assert_eq!(
        final_count, 4,
        "two hard errors after one success must produce exactly 4 messages \
         (2 from turn1 + 1 user delta turn2 + 1 user delta turn3); got {} messages",
        final_count
    );
    assert!(
        final_count >= 2,
        "original turn 1 messages must be preserved; got {}",
        final_count
    );

    Ok(())
}

/// When `on_completion_call` fires on the first LLM call of a fresh session
/// before a CompletionError, the executor persists the user prompt via the delta
/// path — NOT a synthetic placeholder.
///
/// After the fix, `on_completion_call(prompt, history=[])` stores `[prompt]`.
/// `pre_turn_message_count = 0`, so `delta = skip(0) = [prompt]` (non-empty).
/// Delta path fires → 1 message persisted. The placeholder path is never reached.
///
/// This also confirms the `test-hard-error-no-hook-history` session_id is used
/// consistently across this and the renamed test.
#[tokio::test]
async fn hard_error_on_first_llm_call_no_prior_history_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-fresh-session-2";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state = make_memory_state(&temp_dir);

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] (non-empty) → delta path fires → 1 message.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "hard error must propagate as Err to caller"
    );

    // After the fix: 1 message persisted (user prompt via delta path, no placeholder).
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(
        persisted.len(),
        1,
        "fresh session hard error must produce exactly 1 message (user prompt, no placeholder); \
         got {:?}",
        persisted
    );

    assert!(
        matches!(persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be a User message; got {:?}",
        persisted[0]
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// New test: hard error mid-tool-loop preserves real tool results
// ---------------------------------------------------------------------------

/// Verifies the root-cause fix: when a CompletionError occurs on the second LLM
/// sub-call of a multi-tool-call turn, the real tool result from sub-turn 1 is
/// preserved in the session — not replaced by a synthetic "[interrupted]" placeholder.
///
/// Before the fix: `on_completion_call(prompt=tool_result_msg, history=[user_msg,
/// assistant_tool_call])` discarded `prompt` → `last_known_history` = [user_msg,
/// assistant_tool_call] → `inject_missing_tool_results` synthesised "[interrupted]"
/// for "tc1" since no following ToolResult was present.
///
/// After the fix: `last_known_history` = [user_msg, assistant_tool_call,
/// tool_result_msg] → delta includes the real tool result → no synthesis needed.
///
/// Test flow (two `from_stream_turns` turns):
///   Sub-turn 1: LLM → tool_call("tc1", "some_tool", …) + FinalResponse
///               Rig dispatches tool → "some_tool" not in toolset → on_invalid_tool_call
///               → Skip → rig inserts error ToolResult for "tc1" into new_messages
///               → on_completion_call(prompt=tool_result_user_msg,
///                                   history=[user_msg, assistant_tool_call_msg])
///   Sub-turn 2: LLM → error("network failure")
///               → CompletionError → last_known_history snapshot is read
///
/// After fix:
///   last_known_history = [user_msg, assistant_tool_call_msg, tool_result_user_msg]
///   delta = skip(pre_turn_count=0) = all 3 messages
///   inject_missing_tool_results: tool_call "tc1" HAS following ToolResult → no patch
///   persisted = [user_msg, assistant_tool_call_msg, tool_result_user_msg]
///
/// Key assertion: every persisted ToolCall has a following ToolResult NOT containing "[interrupted]".
///
/// The tool must be registered in the ToolServer so rig dispatches it normally.
/// Previously this test relied on `on_invalid_tool_call` → `Skip` to produce a
/// ToolResult, but `Retry` no longer persists the malformed call.
struct SimpleEchoTool;

impl rig::tool::Tool for SimpleEchoTool {
    const NAME: &'static str = "some_tool";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Simple echo tool for testing".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok("real_tool_output".to_string())
    }
}

#[tokio::test]
async fn hard_error_mid_tool_loop_preserves_real_tool_results() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-mid-tool-loop-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: LLM emits tool_call + FinalResponse
    // Turn 2: LLM errors (simulates CompletionError after tool result is in history)
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "some_tool", serde_json::json!({"x": 1})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![MockStreamEvent::error("network failure after tool")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new()
        .tool(SimpleEchoTool)
        .run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![rig::completion::ToolDefinition {
                name: "some_tool".to_string(),
                description: "Simple echo tool for testing".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "do the thing".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // The error must propagate (CompletionError from turn 2, or UnknownToolCall from turn 1)
    assert!(
        result.is_err(),
        "error on sub-call must propagate as Err; got ok"
    );

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // Must have exactly 4 messages:
    //   [User(prompt), Assistant(ToolCall), User(ToolResult), Assistant(close-block)]
    // The 4th is the synthetic assistant message appended by close_open_tool_result_block
    // to prevent user(ToolResult) → user(Text) on the next turn.
    assert_eq!(
        persisted.len(),
        4,
        "mid-tool-loop error must persist exactly [user_msg, assistant_tool_call, tool_result, asst_close]; got {} messages: {:?}",
        persisted.len(),
        persisted
    );
    // The 4th message must be a synthetic assistant close-block.
    assert!(
        matches!(&persisted[3], crate::types::Message::Assistant { .. }),
        "persisted[3] must be the synthetic assistant close-block; got: {:?}",
        persisted[3]
    );

    // For every persisted ToolCall, verify the following ToolResult is NOT "[interrupted]"
    use crate::types::{AssistantContent, UserContent};
    let mut found_tool_call = false;
    for (i, msg) in persisted.iter().enumerate() {
        let crate::types::Message::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter() {
            let AssistantContent::ToolCall(tc) = item else {
                continue;
            };
            let call_id = &tc.id;
            found_tool_call = true;

            // There MUST be a following ToolResult
            let next = persisted.get(i + 1).unwrap_or_else(|| {
                panic!("ToolCall id={call_id} must have a following message in persisted history")
            });

            let result_content = if let crate::types::Message::User { content } = next {
                content
                    .iter()
                    .find_map(|item| {
                        if let UserContent::ToolResult(tr) = item {
                            if &tr.call == call_id {
                                // Extract text content
                                Some(
                                    tr.content
                                        .iter()
                                        .find_map(|c| {
                                            if let crate::types::ToolResultContent::Text(t) = c {
                                                Some(t.text.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_default(),
                                )
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "ToolCall id={call_id} must have a matching ToolResult; \
                             next message: {:?}",
                            next
                        )
                    })
            } else {
                panic!(
                    "Message after ToolCall id={call_id} must be User; got {:?}",
                    next
                )
            };

            // The real key assertion: result must NOT be the synthetic "[interrupted]"
            // placeholder that inject_missing_tool_results would insert.
            assert!(
                !result_content.contains("[interrupted]"),
                "ToolResult id={call_id} must NOT contain '[interrupted]' \
                 (real result was available but got synthetic placeholder); \
                 content: {:?}",
                result_content
            );
        }
    }

    assert!(
        found_tool_call,
        "test requires at least one ToolCall to be persisted; got: {:?}",
        persisted
    );

    Ok(())
}
