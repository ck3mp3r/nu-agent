//! The permission-deny branch: the gate opens the call's row and rig settles it
//! once.
//!
//! A policy denial is a skip, not a stop. The gate publishes `ToolStarted`
//! before the permission check runs, rig reports the skipped outcome, and
//! `on_outcome` closes the row with the denial reason as its result text — so the
//! record stream carries one `Started` and one `Completed`, never a pair plus a
//! settle.

use super::*;

/// The denial reason the resolver returns. It carries the documented
/// `"Permission denied"` prefix.
const DENY_REASON: &str = "Permission denied: test rule";

/// Resolver that denies every call with [`DENY_REASON`].
#[derive(Clone)]
struct DenyAllResolver;

impl crate::hook::permission_resolver::AsyncPermissionResolver for DenyAllResolver {
    async fn resolve(
        &self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        stream: &crate::bus::Stream,
        facts: &crate::bus::FactsTx,
        _cancel: &crate::bus::CancelTx,
    ) -> crate::hook::permission_resolver::PermissionDecision {
        // The gate record carries the card: the denied call's argument-sourced
        // preview rides the record, and the TUI joins it to the call by key.
        let preview = serde_json::from_str::<serde_json::Value>(arguments)
            .ok()
            .and_then(|args| {
                crate::tools::handler::pre_authorize_fs_tool(
                    Some(agnt_kernel::BuiltinKind::Edit),
                    args.as_object()?,
                    std::path::Path::new("/tmp"),
                )
            })
            .and_then(|card| card.preview);
        crate::bus::auto_resolve_permission(
            stream,
            facts,
            agnt_kernel::PermissionRecord::AutoResolved {
                rule_identity: "test rule".to_string(),
                action: agnt_kernel::AutoResolution::Deny,
                reason: Some(DENY_REASON.to_string()),
                call_id: Some(agnt_kernel::ToolCallId::new(tool_name, arguments)),
                shows_preview: preview.is_some(),
                preview,
            },
        );
        let _ = tool_call_id;
        crate::hook::permission_resolver::PermissionDecision::Deny {
            reason: DENY_REASON.to_string(),
        }
    }
}

/// Whether `record` opens a call's row.
fn opens_the_row(record: &SessionRecord) -> bool {
    matches!(record, SessionRecord::Tool(ToolRecord::Started { .. }))
}

/// The denial settlement the assertions read: the closed call's name, status,
/// result text, message, and error kind.
type Settled = (String, ToolStatus, String, Option<String>, Option<String>);

/// The one completion the deny branch settles with: rig's skipped outcome.
fn denial_settlement(events: &[SessionRecord]) -> Result<Settled> {
    let settled: Vec<&ToolRecord> = events
        .iter()
        .filter_map(|event| match event {
            SessionRecord::Tool(record @ ToolRecord::Completed { .. }) => Some(record),
            _ => None,
        })
        .collect();
    let [record] = settled.as_slice() else {
        return Err(format!("a denied call must settle exactly once, got {settled:?}").into());
    };
    let ToolRecord::Completed { call, outcome } = record else {
        return Err(format!("the settlement must be a completion, got {record:?}").into());
    };
    Ok((
        call.name.0.clone(),
        outcome.status,
        outcome.text.clone(),
        outcome.message.clone(),
        outcome
            .kind
            .map(|kind| tool_error_kind_text(kind).to_string()),
    ))
}

/// The text a tool-error kind prints, so a converted assertion can compare the
/// same stable name the old event carried as a string.
fn tool_error_kind_text(kind: agnt_kernel::ToolErrorKind) -> &'static str {
    use agnt_kernel::ToolErrorKind as Kind;
    match kind {
        Kind::InvalidArgs => "invalid_args",
        Kind::Timeout => "timeout",
        Kind::Cancelled => "cancelled",
        Kind::NotFound => "not_found",
        Kind::PermissionDenied => "permission_denied",
        Kind::RateLimited => "rate_limited",
        Kind::Provider => "provider",
        Kind::Network => "network",
        Kind::Other => "other",
        Kind::Denied => "denied",
    }
}

/// A denied call settles once: the gate opens the row with `ToolStarted`, rig
/// reports the skipped outcome, and `on_outcome` closes the row with the denial
/// reason as the result text the drawer renders as the body.
#[tokio::test]
async fn on_dispatch_deny_arm_settles_the_call_once() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let hook = build_hook_with(bus.clone(), DenyAllResolver);
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "add", serde_json::json!({"x": 1, "y": 2})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("done".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ])
    .erase();
    let agent = rig::agent::AgentBuilder::new(model)
        .add_hook(hook)
        .tool(rig::test_utils::MockAddTool)
        .build();
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut stream_rx = bus.stream().subscribe();

    // -- Exec: drive the turn; the deny branch skips the call and the model
    // answers on the next turn.
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}

    // -- Check: one started record, then the one completion that settles it.
    let mut events = Vec::new();
    while let Ok(event) = stream_rx.try_recv() {
        events.push(event);
    }
    assert_eq!(
        tool_event_kinds(&events),
        vec!["started", "completed"],
        "the denied call must publish one started record and exactly one completion"
    );
    assert_eq!(
        events.iter().filter(|event| opens_the_row(event)).count(),
        1,
        "the gate must open the call's row exactly once"
    );
    assert_eq!(
        denial_settlement(&events)?,
        (
            "add".to_string(),
            ToolStatus::Cancelled,
            DENY_REASON.to_string(),
            None,
            None,
        ),
        "the skipped outcome carries the reason as the result text, and nothing else"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The deny branch's args preview
// ---------------------------------------------------------------------------

/// A tool named `edit` so rig dispatches the call. The deny branch skips the
/// call, so the body never runs.
struct MockEditTool;

impl rig::tool::Tool for MockEditTool {
    const NAME: &'static str = "edit";
    type Args = serde_json::Value;
    type Output = String;
    type Error = std::convert::Infallible;

    fn description(&self) -> String {
        "Edits a file".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> core::result::Result<Self::Output, Self::Error> {
        Ok(String::new())
    }
}

/// Drive one scripted turn whose single call is `(tool_name, args)` through a
/// chain built over `permission`, and return every ui event the turn published.
/// The deny branch skips the call, so the model answers on the next turn.
async fn drive_denied_call<P: crate::hook::permission_resolver::AsyncPermissionResolver>(
    permission: P,
    tool_name: &str,
    args: serde_json::Value,
) -> Result<Vec<SessionRecord>> {
    let bus = Bus::default();
    let hook = build_hook_with(bus.clone(), permission);
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", tool_name, args),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("done".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ])
    .erase();
    let agent = rig::agent::AgentBuilder::new(model)
        .add_hook(hook)
        .tool(MockEditTool)
        .tool(rig::test_utils::MockAddTool)
        .build();
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut stream_rx = bus.stream().subscribe();

    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}

    let mut events = Vec::new();
    while let Ok(event) = stream_rx.try_recv() {
        events.push(event);
    }
    Ok(events)
}

/// The ordered tool-event kinds the assertions compare.
fn tool_event_kinds(events: &[SessionRecord]) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionRecord::Tool(ToolRecord::Started { .. }) => Some("started"),
            SessionRecord::Tool(ToolRecord::Completed { .. }) => Some("completed"),
            _ => None,
        })
        .collect()
}

/// The `arguments` string the `ToolStarted` event for `name` carries.
fn started_arguments(events: &[SessionRecord], name: &str) -> Option<String> {
    events.iter().find_map(|event| match event {
        SessionRecord::Tool(ToolRecord::Started { call }) if call.name.0 == name => {
            Some(call.arguments.clone())
        }
        _ => None,
    })
}

/// The `(call key, content)` pairs of every card the turn recorded with content.
fn previews(events: &[SessionRecord]) -> Vec<(String, Option<String>)> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionRecord::Permission(PermissionRecord::Requested {
                call_id, preview, ..
            }) => Some((call_id.as_str().to_string(), preview.clone())),
            SessionRecord::Permission(PermissionRecord::AutoResolved {
                call_id: Some(call_id),
                preview,
                ..
            }) => Some((call_id.as_str().to_string(), preview.clone())),
            _ => None,
        })
        .collect()
}

/// The edit args that form a valid apply-mode operation against `path`.
fn apply_mode_args(path: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "path": path.to_string_lossy(),
        "mode": "apply",
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    })
}

/// A policy denial records the call's argument-sourced preview on the gate
/// record, so the live transcript renders the same diff rehydration builds for
/// the not-run row. The tool settlement carries no preview row: the record is
/// the channel the preview rides.
#[tokio::test]
async fn on_dispatch_deny_arm_emits_preview_for_previewable_call() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = apply_mode_args(&path);

    // -- Exec
    let events = drive_denied_call(DenyAllResolver, "edit", args).await?;

    // -- Check: the settlement carries no preview row; the gate record does.
    assert_eq!(
        tool_event_kinds(&events),
        vec!["started", "completed"],
        "a denied call settles once, and the settlement carries no preview row"
    );

    // -- Check: the preview carries the diff the gate computed, as data.
    let started_args = started_arguments(&events, "edit")
        .ok_or("the denied call must publish ToolStarted with its arguments")?;
    let args: serde_json::Value = serde_json::from_str(&started_args)?;
    let expected = crate::tools::handler::pre_authorize_fs_tool(
        Some(agnt_kernel::BuiltinKind::Edit),
        args.as_object()
            .ok_or("the fixture args must be a JSON object")?,
        std::path::Path::new("/tmp"),
    )
    .ok_or("the fixture args must form a valid apply-mode preview")?
    .preview;
    let previews = previews(&events);
    assert_eq!(
        previews.len(),
        1,
        "the denied call must publish one preview"
    );
    assert_eq!(
        previews[0].0,
        format!("edit\n{started_args}"),
        "the preview key must be the call key"
    );
    assert_eq!(
        previews[0].1, expected,
        "the preview display must be the argument-sourced display"
    );
    Ok(())
}

/// A denied non-Previewable tool publishes no preview: there is no
/// argument-sourced display to build.
#[tokio::test]
async fn on_dispatch_deny_arm_emits_no_preview_for_non_previewable_tool() -> Result<()> {
    // -- Setup & Fixtures
    let args = serde_json::json!({"x": 1, "y": 2});

    // -- Exec
    let events = drive_denied_call(DenyAllResolver, "add", args).await?;

    // -- Check
    assert_eq!(
        tool_event_kinds(&events),
        vec!["started", "completed"],
        "a non-Previewable tool must publish no preview"
    );
    Ok(())
}

/// A denied edit whose args form no valid operation publishes no preview: the
/// argument-sourced display is `None`.
#[tokio::test]
async fn on_dispatch_deny_arm_emits_no_preview_for_invalid_edit_args() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "mode": "apply",
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "world"
        }
    });

    // -- Exec
    let events = drive_denied_call(DenyAllResolver, "edit", args).await?;

    // -- Check
    assert_eq!(
        tool_event_kinds(&events),
        vec!["started", "completed"],
        "a noop edit must publish no preview"
    );
    Ok(())
}
