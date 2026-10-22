//! Translation of the remaining `UiEvent` variants, including the ones with no
//! ACP equivalent.

use agent_client_protocol::schema::v1::{ContentBlock, SessionUpdate};
use agnt_core::protocol::event::{PermissionDecision, ToolDisplay, UiEvent};

use super::support::*;
use crate::translator::EventTranslator;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// AssistantMessage
// ---------------------------------------------------------------------------

#[test]
fn test_translator_assistant_message_produces_agent_message_chunk() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::AssistantMessage {
        delta: "hello".to_string(),
        text: "hello world".to_string(),
    };

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("assistant message must translate")?;

    // -- Check
    let SessionUpdate::AgentMessageChunk(chunk) = update else {
        return Err(format!("expected agent_message_chunk, got: {update:?}").into());
    };
    let ContentBlock::Text(text) = chunk.content else {
        return Err(format!("expected text content, got: {:?}", chunk.content).into());
    };
    assert_eq!(text.text, "hello");
    Ok(())
}

// ---------------------------------------------------------------------------
// LlmCompleted
// ---------------------------------------------------------------------------

#[test]
fn test_translator_llm_completed_produces_usage_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::LlmCompleted {
        response_chars: 10,
        tool_calls: 1,
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: Some(150),
        cost: None,
        cost_complete: false,
    };
    let mut translator = EventTranslator::with_context_size(SESSION, 200_000);

    // -- Exec
    let update = translator
        .translate(event)
        .ok_or("llm completed must translate")?;

    // -- Check
    let SessionUpdate::UsageUpdate(usage) = update else {
        return Err(format!("expected usage_update, got: {update:?}").into());
    };
    assert_eq!(usage.used, 150);
    assert_eq!(usage.size, 200_000);
    assert_eq!(usage.cost, None);
    Ok(())
}

#[test]
fn test_translator_llm_completed_default_context_size_is_builtin_default() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::LlmCompleted {
        response_chars: 0,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: Some(7),
        cost: None,
        cost_complete: false,
    };

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("llm completed must translate")?;

    // -- Check
    let SessionUpdate::UsageUpdate(usage) = update else {
        return Err(format!("expected usage_update, got: {update:?}").into());
    };
    assert_eq!(usage.used, 7);
    assert_eq!(
        usage.size,
        agnt_core::config::defaults::MAX_CONTEXT_TOKENS as u64
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

#[test]
fn test_translator_compaction_started_produces_session_info_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::CompactionStarted {
        source: "proactive".to_string(),
    };

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("compaction started must translate")?;

    // -- Check
    let SessionUpdate::SessionInfoUpdate(info) = update else {
        return Err(format!("expected session_info_update, got: {update:?}").into());
    };
    let meta = info.meta.ok_or("compaction update must carry _meta")?;
    let compaction = meta
        .get("nu-agent")
        .and_then(|value| value.as_object())
        .ok_or("meta must hold the nu-agent compaction object")?;
    assert_eq!(
        compaction.get("state").and_then(|value| value.as_str()),
        Some("started")
    );
    assert_eq!(
        compaction.get("source").and_then(|value| value.as_str()),
        Some("proactive")
    );
    assert_eq!(compaction.get("detail"), None);
    Ok(())
}

#[test]
fn test_translator_compaction_completed_produces_session_info_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::CompactionCompleted {
        source: "proactive".to_string(),
        summary_preview: "summary".to_string(),
        summary_body: "full summary".to_string(),
    };

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("compaction completed must translate")?;

    // -- Check
    let SessionUpdate::SessionInfoUpdate(info) = update else {
        return Err(format!("expected session_info_update, got: {update:?}").into());
    };
    let meta = info.meta.ok_or("compaction update must carry _meta")?;
    let compaction = meta
        .get("nu-agent")
        .and_then(|value| value.as_object())
        .ok_or("meta must hold the nu-agent compaction object")?;
    assert_eq!(
        compaction.get("state").and_then(|value| value.as_str()),
        Some("completed")
    );
    assert_eq!(
        compaction.get("detail").and_then(|value| value.as_str()),
        Some("summary")
    );
    Ok(())
}

#[test]
fn test_translator_compaction_failed_produces_session_info_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::CompactionFailed {
        source: "proactive".to_string(),
        message: "model unavailable".to_string(),
    };

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("compaction failed must translate")?;

    // -- Check
    let SessionUpdate::SessionInfoUpdate(info) = update else {
        return Err(format!("expected session_info_update, got: {update:?}").into());
    };
    let meta = info.meta.ok_or("compaction update must carry _meta")?;
    let compaction = meta
        .get("nu-agent")
        .and_then(|value| value.as_object())
        .ok_or("meta must hold the nu-agent compaction object")?;
    assert_eq!(
        compaction.get("state").and_then(|value| value.as_str()),
        Some("failed")
    );
    assert_eq!(
        compaction.get("detail").and_then(|value| value.as_str()),
        Some("model unavailable")
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Events without an ACP equivalent
// ---------------------------------------------------------------------------

#[test]
fn test_translator_tick_produces_none() -> Result<()> {
    // -- Exec & Check
    assert!(translator().translate(UiEvent::Tick).is_none());
    Ok(())
}

#[test]
fn test_translator_llm_started_produces_none() -> Result<()> {
    // -- Exec & Check
    assert!(translator().translate(UiEvent::LlmStarted).is_none());
    Ok(())
}

#[test]
fn test_translator_tool_preview_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::ToolPreview {
        tool_key: "edit\n{}".to_string(),
        display: ToolDisplay {
            title: Some("edit src/file.txt".to_string()),
            sections: vec![diff_section("src/file.txt", "--- a\n+++ b\n")],
        },
    };

    // -- Exec & Check
    assert!(translator().translate(event).is_none());
    Ok(())
}

#[test]
fn test_translator_warning_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::Warning {
        message: "careful".to_string(),
    };

    // -- Exec & Check
    assert!(translator().translate(event).is_none());
    Ok(())
}

#[test]
fn test_translator_turn_error_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::TurnError {
        message: "boom".to_string(),
    };

    // -- Exec & Check
    assert!(translator().translate(event).is_none());
    Ok(())
}

#[test]
fn test_translator_stopped_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::Stopped {
        reason: "hook stopped the run".to_string(),
    };

    // -- Exec & Check
    assert!(translator().translate(event).is_none());
    Ok(())
}

#[test]
fn test_translator_completed_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::Completed { tool_calls: 3 };

    // -- Exec & Check
    assert!(translator().translate(event).is_none());
    Ok(())
}

#[test]
fn test_translator_compaction_summary_chunk_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::CompactionSummaryChunk {
        source: "proactive".to_string(),
        delta: "sum".to_string(),
        aggregated: "summary".to_string(),
    };

    // -- Exec & Check
    assert!(translator().translate(event).is_none());
    Ok(())
}

#[test]
fn test_translator_permission_requested_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = UiEvent::PermissionRequested {
        request_id: "perm-1".to_string(),
        context: permission_context(),
    };

    // -- Exec & Check
    assert!(translator().translate(event).is_none());
    Ok(())
}

#[test]
fn test_translator_permission_decision_events_produce_none() -> Result<()> {
    // -- Setup & Fixtures
    let events = [
        UiEvent::PermissionDecisionSubmitted {
            request_id: "perm-1".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "rule".to_string(),
        },
        UiEvent::PermissionDecisionTimedOut {
            request_id: "perm-1".to_string(),
        },
        UiEvent::PermissionDecisionIgnored {
            request_id: "perm-1".to_string(),
            reason: "superseded".to_string(),
        },
    ];
    let mut translator = translator();

    // -- Exec & Check
    for event in events {
        assert!(
            translator.translate(event).is_none(),
            "permission decision events must not translate"
        );
    }
    Ok(())
}
