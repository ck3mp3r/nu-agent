//! Translation of the remaining record and narration variants, including the
//! ones with no ACP equivalent.

use agent_client_protocol::schema::v1::{ContentBlock, SessionUpdate};
use agnt_kernel::{
    CompactionRecord, CompactionSource, InferenceRecord, NoticeKind, PermissionRecord, RequestId,
    SessionRecord, StopCause, TurnRecord, Usage,
};
use agnt_runtime::bus::NarrationEvent;
use agnt_runtime::protocol::event::PermissionDecision;

use super::support::*;
use crate::translator::{EventTranslator, TranslateInput};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The `nu-agent` compaction object an `session_info_update` carries.
fn compaction_meta(update: &SessionUpdate) -> Result<serde_json::Map<String, serde_json::Value>> {
    let SessionUpdate::SessionInfoUpdate(info) = update else {
        return Err(format!("expected session_info_update, got: {update:?}").into());
    };
    let meta = info
        .meta
        .as_ref()
        .ok_or("compaction update must carry _meta")?;
    let compaction = meta
        .get("nu-agent")
        .and_then(|value| value.as_object())
        .ok_or("meta must hold the nu-agent compaction object")?;
    Ok(compaction.clone())
}

// ---------------------------------------------------------------------------
// Assistant message
// ---------------------------------------------------------------------------

#[test]
fn test_translator_assistant_message_produces_agent_message_chunk() -> Result<()> {
    // -- Setup & Fixtures: the two fields differ, so the assertion pins which
    // one ACP forwards — the raw delta, not the accumulated text.
    let event = NarrationEvent::InferenceChunk {
        delta: "hello".to_string(),
        aggregated: "hello world".to_string(),
    };

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Narration(&event))
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
// Inference completed
// ---------------------------------------------------------------------------

#[test]
fn test_translator_inference_completed_produces_usage_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Inference(InferenceRecord::Completed {
        usage: Usage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: Some(150),
            cost: None,
            cost_complete: false,
        },
    });
    let mut translator = EventTranslator::with_context_size(SESSION, 200_000);

    // -- Exec
    let update = translator
        .translate(TranslateInput::Record(&event))
        .ok_or("inference completed must translate")?;

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
fn test_translator_inference_completed_default_context_size_is_builtin_default() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Inference(InferenceRecord::Completed {
        usage: Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: Some(7),
            cost: None,
            cost_complete: false,
        },
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("inference completed must translate")?;

    // -- Check
    let SessionUpdate::UsageUpdate(usage) = update else {
        return Err(format!("expected usage_update, got: {update:?}").into());
    };
    assert_eq!(usage.used, 7);
    assert_eq!(
        usage.size,
        agnt_runtime::config::defaults::MAX_CONTEXT_TOKENS as u64
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

/// The compaction label this translator owns, one case per source variant.
#[test]
fn test_translator_compaction_started_labels_each_source() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (CompactionSource::Auto, "auto"),
        (CompactionSource::Manual, "slash"),
    ];
    let mut translator = translator();

    // -- Exec & Check
    for (source, expected) in cases {
        let event = SessionRecord::Compaction(CompactionRecord::Started { source });
        let update = translator
            .translate(TranslateInput::Record(&event))
            .ok_or("compaction started must translate")?;
        let compaction = compaction_meta(&update)?;
        assert_eq!(
            compaction.get("state").and_then(|value| value.as_str()),
            Some("started")
        );
        assert_eq!(
            compaction.get("source").and_then(|value| value.as_str()),
            Some(expected),
            "the source label is this translator's, not the producer's"
        );
        assert_eq!(compaction.get("detail"), None);
    }
    Ok(())
}

#[test]
fn test_translator_compaction_completed_produces_session_info_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Compaction(CompactionRecord::Completed {
        source: CompactionSource::Auto,
        summary: "summary".to_string(),
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("compaction completed must translate")?;

    // -- Check
    let compaction = compaction_meta(&update)?;
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
fn test_translator_uncompacted_produces_session_info_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Compaction(CompactionRecord::Uncompacted {
        source: CompactionSource::Manual,
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("an uncompacted record must translate")?;

    // -- Check
    let compaction = compaction_meta(&update)?;
    assert_eq!(
        compaction.get("state").and_then(|value| value.as_str()),
        Some("undone")
    );
    assert_eq!(
        compaction.get("source").and_then(|value| value.as_str()),
        Some("slash")
    );
    assert_eq!(compaction.get("detail"), None);
    Ok(())
}

#[test]
fn test_translator_compaction_completed_detail_is_a_preview_not_the_body() -> Result<()> {
    // -- Setup & Fixtures: a summary longer than the preview window.
    let summary = "s".repeat(500);
    let event = SessionRecord::Compaction(CompactionRecord::Completed {
        source: CompactionSource::Auto,
        summary: summary.clone(),
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("compaction completed must translate")?;

    // -- Check
    let compaction = compaction_meta(&update)?;
    let detail = compaction
        .get("detail")
        .and_then(|value| value.as_str())
        .ok_or("a completed compaction must carry its preview")?;
    assert_eq!(
        detail.chars().count(),
        200,
        "the client sees a preview only"
    );
    Ok(())
}

#[test]
fn test_translator_compaction_failed_produces_session_info_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Compaction(CompactionRecord::Failed {
        source: CompactionSource::Auto,
        message: "model unavailable".to_string(),
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("compaction failed must translate")?;

    // -- Check
    let compaction = compaction_meta(&update)?;
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
// Warning
// ---------------------------------------------------------------------------

#[test]
fn test_translator_warning_produces_agent_message_chunk() -> Result<()> {
    // -- Setup & Fixtures
    let event = NarrationEvent::SystemMessage {
        kind: NoticeKind::Warning,
        text: "Conversation is using ~120000 estimated tokens".to_string(),
    };

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Narration(&event))
        .ok_or("warning must translate")?;

    // -- Check
    let SessionUpdate::AgentMessageChunk(chunk) = update else {
        return Err(format!("expected agent_message_chunk, got: {update:?}").into());
    };
    let ContentBlock::Text(text) = chunk.content else {
        return Err(format!("expected text content, got: {:?}", chunk.content).into());
    };
    assert_eq!(text.text, "Conversation is using ~120000 estimated tokens");
    Ok(())
}

#[test]
fn test_translator_warning_empty_message_produces_agent_message_chunk() -> Result<()> {
    // -- Setup & Fixtures
    let event = NarrationEvent::SystemMessage {
        kind: NoticeKind::Warning,
        text: String::new(),
    };

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Narration(&event))
        .ok_or("empty warning must still translate")?;

    // -- Check
    let SessionUpdate::AgentMessageChunk(chunk) = update else {
        return Err(format!("expected agent_message_chunk, got: {update:?}").into());
    };
    let ContentBlock::Text(text) = chunk.content else {
        return Err(format!("expected text content, got: {:?}", chunk.content).into());
    };
    assert_eq!(text.text, "");
    Ok(())
}

// ---------------------------------------------------------------------------
// Variants without an ACP equivalent
// ---------------------------------------------------------------------------

#[test]
fn test_translator_inference_started_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Inference(InferenceRecord::Started);

    // -- Exec & Check
    assert!(
        translator()
            .translate(TranslateInput::Record(&event))
            .is_none()
    );
    Ok(())
}

#[test]
fn test_translator_turn_error_produces_none() -> Result<()> {
    // -- Setup & Fixtures: a turn error reaches the client as the prompt
    // response, never as a transcript chunk.
    let event = NarrationEvent::SystemMessage {
        kind: NoticeKind::TurnError,
        text: "boom".to_string(),
    };

    // -- Exec & Check
    assert!(
        translator()
            .translate(TranslateInput::Narration(&event))
            .is_none()
    );
    Ok(())
}

#[test]
fn test_translator_stopped_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Inference(InferenceRecord::Stopped {
        cause: StopCause::Other,
        reason: "hook stopped the run".to_string(),
    });

    // -- Exec & Check
    assert!(
        translator()
            .translate(TranslateInput::Record(&event))
            .is_none()
    );
    Ok(())
}

#[test]
fn test_translator_turn_completed_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 });

    // -- Exec & Check
    assert!(
        translator()
            .translate(TranslateInput::Record(&event))
            .is_none()
    );
    Ok(())
}

#[test]
fn test_translator_compaction_summary_chunk_produces_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Compaction(CompactionRecord::SummaryChunk {
        source: CompactionSource::Auto,
        delta: "sum".to_string(),
        aggregated: "summary".to_string(),
    });

    // -- Exec & Check
    assert!(
        translator()
            .translate(TranslateInput::Record(&event))
            .is_none()
    );
    Ok(())
}

#[test]
fn test_translator_permission_requested_produces_none() -> Result<()> {
    // -- Setup & Fixtures: the permission bridge owns the card, not the
    // translator.
    let event = SessionRecord::Permission(default_requested());

    // -- Exec & Check
    assert!(
        translator()
            .translate(TranslateInput::Record(&event))
            .is_none()
    );
    Ok(())
}

#[test]
fn test_translator_permission_resolutions_produce_none() -> Result<()> {
    // -- Setup & Fixtures: every resolution of the gate is silent on the wire.
    let events = [
        PermissionRecord::Decided {
            request_id: RequestId::from_minted("perm-1"),
            decision: PermissionDecision::AllowOnce,
            rule_identity: "rule".to_string(),
        },
        PermissionRecord::TimedOut {
            request_id: RequestId::from_minted("perm-1"),
        },
        PermissionRecord::Ignored {
            request_id: RequestId::from_minted("perm-1"),
            reason: "superseded".to_string(),
        },
    ];
    let mut translator = translator();

    // -- Exec & Check
    for record in events {
        let event = SessionRecord::Permission(record);
        assert!(
            translator
                .translate(TranslateInput::Record(&event))
                .is_none(),
            "permission resolutions must not translate"
        );
    }
    Ok(())
}
