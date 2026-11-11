//! The `AutoResolved` fold: a policy-allowed call paints the card the gate fold
//! paints for the same call.
//!
//! An auto-accepted call has no `Requested` record — the policy resolved it
//! without prompting — so its `AutoResolved` record is the only carrier of the
//! card's trigger. The drawer derives the card itself, from the exact call key
//! the record carries and the content only the producer can supply.
//!
//! The negatives: a denial, a call with no card, a record that names no call,
//! and a call nothing is tracking under the key all paint nothing.

use crate::interaction::reducer::{RenderInput, reduce};
use crate::state::AppState;
use crate::test_support::{fold_record, tool_completed, tool_started};
use agnt_render::transcript::entry::TranscriptEntry;
use agnt_render::transcript::ir::{ContentKind, Display};
use agnt_runtime::protocol::events::{
    AutoResolution, PermissionRecord, RequestId, SessionRecord, ToolCall, ToolCallId, ToolOutcome,
};
use agnt_runtime::session::tool_status::ToolStatus;
use agnt_runtime::tools::handler::ToolSource;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const NU_ARGS: &str = r#"{"command":"ls"}"#;
const EDIT_ARGS: &str = r#"{"path":"a.rs"}"#;
const EDIT_DIFF: &str = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n";

/// The nu call the fixtures start.
fn nu_call() -> ToolCall {
    ToolCall::new("nu", ToolSource::Builtin, NU_ARGS)
}

/// The edit call the fixtures start.
fn edit_call() -> ToolCall {
    ToolCall::new("edit", ToolSource::Builtin, EDIT_ARGS)
}

/// The exact key of the call the policy resolved. The gate carries this key,
/// not the call's parts.
fn nu_key() -> ToolCallId {
    ToolCallId::from_key(format!("nu\n{NU_ARGS}"))
}

fn edit_key() -> ToolCallId {
    ToolCallId::from_key(format!("edit\n{EDIT_ARGS}"))
}

/// The policy's word on a call: the record the runtime publishes when a rule
/// resolved it without prompting.
fn auto_resolved(
    action: AutoResolution,
    call_id: Option<ToolCallId>,
    shows_preview: bool,
    preview: Option<&str>,
) -> SessionRecord {
    SessionRecord::Permission(PermissionRecord::AutoResolved {
        rule_identity: "closure:nu.command:*".to_string(),
        action,
        reason: None,
        call_id,
        shows_preview,
        preview: preview.map(str::to_string),
    })
}

/// The card the gate fold paints for the call the policy resolved: the same
/// key, the same producer content.
fn gate_record(shows_preview: bool, preview: Option<&str>) -> SessionRecord {
    SessionRecord::Permission(PermissionRecord::Requested {
        request_id: RequestId::from_minted("ask-0000000000000001"),
        call_id: nu_key(),
        tool: "nu".to_string(),
        source: "closure".to_string(),
        rule_identity: "closure:nu.command:*".to_string(),
        scope: "closure".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        mode: Some("ask".to_string()),
        summary: "ls".to_string(),
        shows_preview,
        preview: preview.map(str::to_string),
    })
}

/// The preview of the transcript's only tool entry: the started call's card,
/// never a companion row.
fn only_tool_preview(state: &AppState) -> Result<Option<Display>> {
    let mut tools = state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Tool(tool) => Some(tool),
            _ => None,
        });
    let tool = tools
        .next()
        .ok_or("expected the started call's tool entry")?;
    if tools.next().is_some() {
        return Err("the card attaches to the started call; no row is pushed".into());
    }
    Ok(tool.preview.clone())
}

#[test]
fn test_interaction_auto_resolved_card_allow_derives_nu_card() -> Result<()> {
    // -- Setup & Fixtures: the call is tracked and in progress when the policy
    // resolves it.
    let mut state = AppState::default();
    let call = nu_call();
    fold_record(&mut state, tool_started(call.clone()));

    // -- Exec
    let changed = fold_record(
        &mut state,
        auto_resolved(AutoResolution::Allow, Some(nu_key()), true, None),
    );

    // -- Check: the card derives from the call's own arguments.
    assert!(
        changed,
        "attaching the card is a change the surface must paint"
    );
    let preview = only_tool_preview(&state)?
        .ok_or("an auto-allowed nu call should carry the card its arguments name")?;
    let section = preview
        .sections
        .first()
        .ok_or("the nu card holds one code section")?;
    assert_eq!(section.content, "ls");
    assert_eq!(
        section.kind,
        ContentKind::Code {
            language: "nu".to_string()
        }
    );
    Ok(())
}

/// The policy fold and the gate fold derive the same card for the same call:
/// the record carries the key and the producer content, nothing else.
#[test]
fn test_interaction_auto_resolved_card_matches_gate_card() -> Result<()> {
    // -- Setup & Fixtures: one started nu call behind a policy record, one
    // behind a gate record — the same call key on both.
    let mut auto = AppState::default();
    fold_record(&mut auto, tool_started(nu_call()));
    let mut gate = AppState::default();
    fold_record(&mut gate, tool_started(nu_call()));

    // -- Exec
    fold_record(
        &mut auto,
        auto_resolved(AutoResolution::Allow, Some(nu_key()), true, None),
    );
    let _ = reduce(
        &mut gate,
        RenderInput::Record(&gate_record(true, None)),
        None,
    );

    // -- Check
    let auto_card = only_tool_preview(&auto)?;
    let gate_card = only_tool_preview(&gate)?;
    assert!(gate_card.is_some(), "the gate fold paints the card");
    assert_eq!(
        auto_card, gate_card,
        "the policy fold paints the same card the gate fold paints"
    );
    Ok(())
}

/// An auto-allowed edit carries the producer's diff, exactly as the gate fold
/// does: the drawer derives the card over the path the call names.
#[test]
fn test_interaction_auto_resolved_card_allow_attaches_edit_diff() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(edit_call()));

    // -- Exec
    let changed = fold_record(
        &mut state,
        auto_resolved(
            AutoResolution::Allow,
            Some(edit_key()),
            true,
            Some(EDIT_DIFF),
        ),
    );

    // -- Check
    assert!(changed, "attaching the diff card is a change");
    let preview =
        only_tool_preview(&state)?.ok_or("the auto-allowed edit should carry its diff card")?;
    let section = preview
        .sections
        .first()
        .ok_or("the edit card holds one diff section")?;
    assert_eq!(section.content, EDIT_DIFF);
    assert_eq!(
        section.kind,
        ContentKind::Diff {
            language: "diff".to_string(),
            path: "a.rs".to_string(),
        }
    );
    Ok(())
}

/// A policy denial renders no card, even when it carries the call key.
#[test]
fn test_interaction_auto_resolved_card_deny_renders_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(nu_call()));

    // -- Exec
    let changed = fold_record(
        &mut state,
        auto_resolved(AutoResolution::Deny, Some(nu_key()), true, None),
    );

    // -- Check
    assert!(!changed, "a denial folds as a no-op");
    assert!(
        only_tool_preview(&state)?.is_none(),
        "a denied call paints no card"
    );
    Ok(())
}

/// A call the policy reports as cardless renders no card.
#[test]
fn test_interaction_auto_resolved_card_no_preview_renders_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(nu_call()));

    // -- Exec
    let changed = fold_record(
        &mut state,
        auto_resolved(AutoResolution::Allow, Some(nu_key()), false, None),
    );

    // -- Check
    assert!(!changed, "a cardless call folds as a no-op");
    assert!(
        only_tool_preview(&state)?.is_none(),
        "the flag says whether a card exists at all"
    );
    Ok(())
}

/// A record that names no call renders no card.
#[test]
fn test_interaction_auto_resolved_card_no_call_id_renders_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(nu_call()));

    // -- Exec
    let changed = fold_record(
        &mut state,
        auto_resolved(AutoResolution::Allow, None, true, None),
    );

    // -- Check
    assert!(!changed, "a record naming no call folds as a no-op");
    assert!(
        only_tool_preview(&state)?.is_none(),
        "no card is attached to a call the record does not name"
    );
    Ok(())
}

/// The attach no-ops when nothing is in progress under the key: the call has
/// already settled, so there is no pending row to paint.
#[test]
fn test_interaction_auto_resolved_card_settled_call_renders_nothing() -> Result<()> {
    // -- Setup & Fixtures: the call ran and completed before the policy record
    // folds.
    let mut state = AppState::default();
    let call = nu_call();
    fold_record(&mut state, tool_started(call.clone()));
    fold_record(
        &mut state,
        tool_completed(
            call,
            ToolOutcome {
                status: ToolStatus::Success,
                payload: None,
                text: "out".to_string(),
                message: None,
                kind: None,
            },
        ),
    );

    // -- Exec
    let changed = fold_record(
        &mut state,
        auto_resolved(AutoResolution::Allow, Some(nu_key()), true, None),
    );

    // -- Check
    assert!(!changed, "a settled call has no row to paint");
    assert!(
        only_tool_preview(&state)?.is_none(),
        "the settled call keeps no card"
    );
    Ok(())
}
