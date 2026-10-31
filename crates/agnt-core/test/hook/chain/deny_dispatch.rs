//! The permission-deny branch's `Started` + `Completed(success=false)` pair.
//!
//! The deny branch publishes the pair itself because the call never runs. rig
//! then closes the denied dispatch's scope, so `on_outcome` reports the
//! skipped result as a second `Completed` — the test pins both, in order.

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
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _bus: &Bus,
    ) -> crate::hook::permission_resolver::PermissionDecision {
        crate::hook::permission_resolver::PermissionDecision::Deny {
            reason: DENY_REASON.to_string(),
        }
    }
}

/// The tool-event shape the assertions compare: `(name, success, error_kind,
/// message)`, with the last three `None` for a `ToolStarted`.
type ToolEventSummary = (String, Option<bool>, Option<String>, Option<String>);

fn tool_event_summary(event: &SystemEvent) -> Option<ToolEventSummary> {
    match event {
        SystemEvent::ToolStarted { call } => Some((call.name.0.clone(), None, None, None)),
        SystemEvent::ToolCompleted { call, outcome } => Some((
            call.name.0.clone(),
            Some(outcome.success),
            outcome.error_kind.clone(),
            outcome.message.clone(),
        )),
        _ => None,
    }
}

/// The deny branch publishes `ToolStarted` then `ToolCompleted(success=false)`
/// carrying `error_kind: Some("denied")` and the resolver's reason as the
/// message — the pair the live transcript renders as the not-run ⊘ block.
#[tokio::test]
async fn on_dispatch_deny_arm_emits_pair_with_denied_kind() -> Result<()> {
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
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec: drive the turn; the deny branch skips the call and the model
    // answers on the next turn.
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}

    // -- Check: the deny branch's pair, then rig's skipped-result report.
    let mut tool_events = Vec::new();
    while let Ok(event) = ui_rx.try_recv() {
        if let Some(summary) = tool_event_summary(&event) {
            tool_events.push(summary);
        }
    }
    assert_eq!(
        tool_events,
        vec![
            ("add".to_string(), None, None, None),
            (
                "add".to_string(),
                Some(false),
                Some("denied".to_string()),
                Some(DENY_REASON.to_string()),
            ),
            // rig closes the denied dispatch's scope, so `on_outcome` reports
            // the skipped result too: no kind, no message.
            ("add".to_string(), Some(false), None, None),
        ],
        "the deny branch must publish its Started+Completed pair with the denied kind and the reason"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The deny branch's args preview
// ---------------------------------------------------------------------------

/// Resolver that denies every call and reports the preview as already shown —
/// the Ask-path shape, where the permission card anchored the preview before
/// the denial.
#[derive(Clone)]
struct DenyAllPreviewedResolver;

impl crate::hook::permission_resolver::AsyncPermissionResolver for DenyAllPreviewedResolver {
    async fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _bus: &Bus,
    ) -> crate::hook::permission_resolver::PermissionDecision {
        crate::hook::permission_resolver::PermissionDecision::Deny {
            reason: DENY_REASON.to_string(),
        }
    }

    fn take_previewed(&self, _tool_name: &str, _arguments: &str) -> bool {
        true
    }
}

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
) -> Result<Vec<SystemEvent>> {
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
    let mut ui_rx = bus.ui_event().subscribe();

    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}

    let mut events = Vec::new();
    while let Ok(event) = ui_rx.try_recv() {
        events.push(event);
    }
    Ok(events)
}

/// The ordered tool-event kinds the assertions compare.
fn tool_event_kinds(events: &[SystemEvent]) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|event| match event {
            SystemEvent::ToolStarted { .. } => Some("started"),
            SystemEvent::PreviewRequested { .. } => Some("preview"),
            SystemEvent::ToolCompleted { .. } => Some("completed"),
            _ => None,
        })
        .collect()
}

/// The `arguments` string the `ToolStarted` event for `name` carries.
fn started_arguments(events: &[SystemEvent], name: &str) -> Option<String> {
    events.iter().find_map(|event| match event {
        SystemEvent::ToolStarted { call } if call.name.0 == name => Some(call.arguments.clone()),
        _ => None,
    })
}

/// The `(tool_key, display)` pairs of every `ToolPreview` the turn published.
fn previews(events: &[SystemEvent]) -> Vec<(String, crate::protocol::event::ToolDisplay)> {
    events
        .iter()
        .filter_map(|event| match event {
            SystemEvent::PreviewRequested { call, display } => {
                Some((call.id.as_str().to_string(), display.clone()))
            }
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

/// A policy denial publishes the call's argument-sourced preview between
/// `ToolStarted` and `ToolCompleted`, so the live transcript renders the same
/// diff rehydration builds for the not-run row.
#[tokio::test]
async fn on_dispatch_deny_arm_emits_preview_for_previewable_call() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = apply_mode_args(&path);

    // -- Exec
    let events = drive_denied_call(DenyAllResolver, "edit", args).await?;

    // -- Check: the preview lands between the pair's two sends.
    assert_eq!(
        tool_event_kinds(&events),
        vec!["started", "preview", "completed", "completed"],
        "the deny branch must publish the preview between Started and Completed"
    );

    // -- Check: the preview carries the flush path's event shape.
    let started_args = started_arguments(&events, "edit")
        .ok_or("the denied call must publish ToolStarted with its arguments")?;
    let expected = crate::tools::handler::tool_display_from_arguments(
        "edit",
        &started_args,
        std::path::Path::new("/tmp"),
    )
    .ok_or("the fixture args must form a valid apply-mode preview")?;
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
        vec!["started", "completed", "completed"],
        "a non-Previewable tool must publish no preview"
    );
    Ok(())
}

/// A denial whose preview the permission card already anchored publishes no
/// second copy: `take_previewed` returning true suppresses the emission.
#[tokio::test]
async fn on_dispatch_deny_arm_emits_no_preview_when_already_previewed() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = apply_mode_args(&path);

    // -- Exec
    let events = drive_denied_call(DenyAllPreviewedResolver, "edit", args).await?;

    // -- Check
    assert_eq!(
        tool_event_kinds(&events),
        vec!["started", "completed", "completed"],
        "an already-previewed denial must publish no second preview"
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
        vec!["started", "completed", "completed"],
        "a noop edit must publish no preview"
    );
    Ok(())
}
