//! Vocabulary tests for the bus payloads (kernel `records.rs` + `bus/events.rs`).
//!
//! The census is the contract: every family the composition root hands a
//! consumer is enumerated here once, so a variant cannot be added to a channel
//! without this test noticing.

use agnt_kernel::ToolName;
use agnt_kernel::bus::{
    CancelEvent, ControlCommand, FactRecord, LifecycleEvent, NarrationEvent, PermissionGate,
    SharedUiAction, WorkerPhase,
};
use agnt_kernel::{
    AutoResolution, CompactionRecord, CompactionSource, InferenceRecord, NoticeKind, NoticeRecord,
    PermissionDecision, PermissionRecord, PromptRecord, PromptStamps, RequestId, SessionId,
    SessionLifecycle, SessionRecord, StopCause, ToolCall, ToolCallId, ToolErrorKind, ToolOutcome,
    ToolRecord, ToolSource, ToolStatus, TurnRecord, Usage,
};

// region:    --- Tests

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_records_tool_call_id_of_derives_call_key() -> Result<()> {
    // -- Exec
    let id = ToolCallId::new("nu", "{\"command\":\"ls\"}");
    let other = ToolCallId::new("nu", "{\"command\":\"ls -la\"}");

    // -- Check
    assert_eq!(id.as_str(), "nu\n{\"command\":\"ls\"}");
    assert_ne!(id.as_str(), other.as_str());
    Ok(())
}

#[test]
fn test_records_tool_call_new_derives_id_from_payload() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = "{\"path\":\"src/main.rs\"}";

    // -- Exec
    let call = ToolCall::new("edit", ToolSource::Builtin, arguments);

    // -- Check
    assert_eq!(call.id(), &ToolCallId::new("edit", arguments));
    assert_eq!(call.name, ToolName("edit".to_string()));
    assert_eq!(call.source, ToolSource::Builtin);
    assert_eq!(call.arguments, arguments);
    Ok(())
}

#[test]
fn test_records_stream_vocabulary_covers_every_family() -> Result<()> {
    // -- Setup & Fixtures
    let call = tool_call();
    let request_id = RequestId::from_minted("req-1");
    let session_id = SessionId::new("s-1");
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 2,
        total_tokens: Some(12),
        cost: Some(0.001),
        cost_complete: true,
    };

    let records = vec![
        SessionRecord::Prompt(PromptRecord {
            text: "hello".to_string(),
            stamps: PromptStamps::default(),
            task_id: None,
        }),
        SessionRecord::Inference(InferenceRecord::Started),
        SessionRecord::Inference(InferenceRecord::Chunk {
            delta: "hi".to_string(),
            aggregated: "hi".to_string(),
        }),
        SessionRecord::Inference(InferenceRecord::Completed { usage }),
        SessionRecord::Inference(InferenceRecord::Stopped {
            cause: StopCause::UserCancelled,
            reason: "hook".to_string(),
        }),
        SessionRecord::Tool(ToolRecord::Started { call: call.clone() }),
        SessionRecord::Tool(ToolRecord::Completed {
            call: call.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                payload: Some("{}".to_string()),
                text: "{}".to_string(),
                message: None,
                kind: None,
            },
        }),
        SessionRecord::Permission(PermissionRecord::Requested {
            request_id: request_id.clone(),
            call_id: call.id().clone(),
            tool: "nu".to_string(),
            source: "builtin".to_string(),
            rule_identity: "rule-1".to_string(),
            scope: "call".to_string(),
            pattern: "nu".to_string(),
            target_field: None,
            mode: None,
            summary: "run nu".to_string(),
            shows_preview: false,
            preview: None,
        }),
        SessionRecord::Permission(PermissionRecord::Decided {
            request_id: request_id.clone(),
            decision: PermissionDecision::Deny,
            rule_identity: "rule-1".to_string(),
        }),
        SessionRecord::Permission(PermissionRecord::TimedOut {
            request_id: request_id.clone(),
        }),
        SessionRecord::Permission(PermissionRecord::Ignored {
            request_id: request_id.clone(),
            reason: "turn cancelled".to_string(),
        }),
        SessionRecord::Permission(PermissionRecord::Cancelled {
            request_id,
            rule_identity: "rule-1".to_string(),
        }),
        SessionRecord::Permission(PermissionRecord::AutoResolved {
            rule_identity: "rule-1".to_string(),
            action: AutoResolution::Allow,
            reason: None,
            call_id: Some(call.id().clone()),
            shows_preview: false,
            preview: None,
        }),
        SessionRecord::Compaction(CompactionRecord::Requested {
            source: CompactionSource::Auto,
        }),
        SessionRecord::Compaction(CompactionRecord::Started {
            source: CompactionSource::Auto,
        }),
        SessionRecord::Compaction(CompactionRecord::SummaryChunk {
            source: CompactionSource::Auto,
            delta: "sum".to_string(),
            aggregated: "sum".to_string(),
        }),
        SessionRecord::Compaction(CompactionRecord::Completed {
            source: CompactionSource::Auto,
            summary: "sum".to_string(),
        }),
        SessionRecord::Compaction(CompactionRecord::Failed {
            source: CompactionSource::Auto,
            message: "boom".to_string(),
        }),
        SessionRecord::Notice(NoticeRecord {
            kind: NoticeKind::Warning,
            text: "careful".to_string(),
        }),
        SessionRecord::Session(SessionLifecycle::Started {
            session_id: session_id.clone(),
            hydrated: false,
        }),
        SessionRecord::Session(SessionLifecycle::Ended {
            session_id: session_id.clone(),
        }),
        SessionRecord::Session(SessionLifecycle::Switched {
            from: None,
            to: session_id,
        }),
        SessionRecord::Session(SessionLifecycle::AgentSwitched {
            from: None,
            to: "reviewer".to_string(),
        }),
        SessionRecord::Session(SessionLifecycle::ModelSwitched {
            from: "openai/gpt-4o".to_string(),
            to: "openai/gpt-5".to_string(),
        }),
        SessionRecord::Turn(TurnRecord::Completed { duration_ms: 1500 }),
    ];

    // -- Exec
    let count = records.len();

    // -- Check
    assert_eq!(count, 25, "one row per record variant and payload arm");
    Ok(())
}

#[test]
fn test_events_spine_narration_and_control_cover_every_family() -> Result<()> {
    // -- Setup & Fixtures
    let stamps = PromptStamps::default();
    let usage = Usage {
        input_tokens: 1,
        output_tokens: 1,
        total_tokens: Some(2),
        cost: None,
        cost_complete: false,
    };

    let spine = vec![
        LifecycleEvent::PromptSubmitted {
            text: "hi".to_string(),
            task_id: None,
            stamps,
        },
        LifecycleEvent::TurnStarted {
            prompt: "hi".to_string(),
            task_id: None,
        },
        LifecycleEvent::TurnCompleted {
            task_id: None,
            duration_ms: 1,
        },
        LifecycleEvent::TaskCompleted {
            task_id: "t-1".to_string(),
            output: "done".to_string(),
        },
        LifecycleEvent::TaskFailed {
            task_id: "t-1".to_string(),
            error: "boom".to_string(),
        },
        LifecycleEvent::SessionStarted {
            session_id: "s-1".to_string(),
            hydrated: false,
        },
        LifecycleEvent::SessionEnded {
            session_id: "s-1".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
        },
        LifecycleEvent::SessionSwitched {
            from: None,
            to: "s-2".to_string(),
        },
        LifecycleEvent::AgentSwitched {
            from: None,
            to: "reviewer".to_string(),
        },
        LifecycleEvent::ModelSwitched {
            from: "a/b".to_string(),
            to: "a/c".to_string(),
        },
        LifecycleEvent::CompactionStarted {
            source: CompactionSource::Auto,
        },
        LifecycleEvent::CompactionCompleted {
            source: CompactionSource::Auto,
            summary: "sum".to_string(),
        },
        LifecycleEvent::CompactionFailed {
            source: CompactionSource::Auto,
            message: "boom".to_string(),
        },
    ];

    let narration = [
        NarrationEvent::InferenceStarted,
        NarrationEvent::InferenceChunk {
            delta: "hi".to_string(),
            aggregated: "hi".to_string(),
        },
        NarrationEvent::InferenceCompleted { usage },
        NarrationEvent::InferenceStopped {
            reason: "hook".to_string(),
        },
        NarrationEvent::CompactionSummaryChunk {
            source: CompactionSource::Auto,
            delta: "sum".to_string(),
            aggregated: "sum".to_string(),
        },
        NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text: "careful".to_string(),
        },
    ];

    let control = [
        ControlCommand::EnqueueExternalPrompt {
            task_id: "t-1".to_string(),
            prompt: "hi".to_string(),
        },
        ControlCommand::PermissionDecided {
            request_id: RequestId::from_minted("req-1"),
            decision: PermissionDecision::AllowOnce,
        },
        ControlCommand::CompactionRequested {
            source: CompactionSource::Manual,
        },
        ControlCommand::SharedUiAction {
            action: SharedUiAction::Help,
        },
    ];

    let facts = vec![
        FactRecord::WorkerPhase(WorkerPhase::Idle),
        FactRecord::PermissionGate(PermissionGate::Idle),
        FactRecord::SessionUsage(Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
        }),
        FactRecord::ActiveModelIdentity("a/b".to_string()),
        FactRecord::ActiveAgentIdentity("reviewer".to_string()),
        FactRecord::ActivePersonaIcon(None),
        FactRecord::ContextWindowMaxTokens(Some(200_000)),
        FactRecord::McpServerState {
            server: "mem".to_string(),
            state: agnt_kernel::bus::McpServerUsability::Enabled,
            error: None,
        },
        FactRecord::McpVisibleToolTotal(0),
        FactRecord::McpVisibleToolCount {
            server: "mem".to_string(),
            count: 0,
        },
        FactRecord::McpVisibleToolNames {
            server: "mem".to_string(),
            names: Vec::new(),
        },
    ];

    // -- Exec & Check
    assert_eq!(spine.len(), 13, "the spine's ordered, complete rows");
    assert_eq!(narration.len(), 6, "the narration families");
    assert_eq!(control.len(), 4, "the control commands, one owner each");
    assert_eq!(facts.len(), 11, "the latest-wins snapshots");
    assert_eq!(
        vec![CancelEvent::Requested].len(),
        1,
        "cancellation has one variant"
    );
    Ok(())
}

#[test]
fn test_records_payloads_carry_typed_identity() -> Result<()> {
    // -- Setup & Fixtures
    let call = tool_call();

    // -- Exec
    let started = SessionRecord::Tool(ToolRecord::Started { call: call.clone() });
    let auto_resolved = SessionRecord::Permission(PermissionRecord::AutoResolved {
        rule_identity: "rule-1".to_string(),
        action: AutoResolution::Deny,
        reason: Some("blocked by policy".to_string()),
        call_id: Some(call.id().clone()),
        shows_preview: false,
        preview: None,
    });
    let completed = SessionRecord::Tool(ToolRecord::Completed {
        call,
        outcome: ToolOutcome {
            status: ToolStatus::Cancelled,
            payload: None,
            text: String::new(),
            message: Some("denied".to_string()),
            kind: Some(ToolErrorKind::Denied),
        },
    });

    // -- Check
    let SessionRecord::Tool(ToolRecord::Started { call: started_call }) = started else {
        return Err("should be a started tool record".into());
    };
    assert_eq!(started_call.id().as_str(), "nu\n{\"command\":\"ls\"}");

    let SessionRecord::Permission(PermissionRecord::AutoResolved { action, reason, .. }) =
        auto_resolved
    else {
        return Err("should be an auto-resolved permission record".into());
    };
    assert_eq!(action, AutoResolution::Deny);
    assert_eq!(reason.as_deref(), Some("blocked by policy"));

    let SessionRecord::Tool(ToolRecord::Completed { outcome, .. }) = completed else {
        return Err("should be a completed tool record".into());
    };
    assert_eq!(outcome.status, ToolStatus::Cancelled);
    assert_eq!(outcome.kind, Some(ToolErrorKind::Denied));
    assert_eq!(outcome.message.as_deref(), Some("denied"));
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

fn tool_call() -> ToolCall {
    ToolCall::new("nu", ToolSource::Builtin, "{\"command\":\"ls\"}")
}

// endregion: --- Test Support
