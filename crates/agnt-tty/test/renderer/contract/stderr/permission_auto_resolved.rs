//! The stderr renderer's `permission_auto_resolved` line.

use super::super::super::StderrUiRenderer;
use crate::renderer::{RenderInput, UiRenderer};
use agnt_kernel::{AutoResolution, PermissionRecord, SessionRecord};

use crate::policy::{UiPolicy, Verbosity};

#[test]
fn permission_auto_resolved_renders_line_and_quiet_suppresses()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let allow = SessionRecord::Permission(PermissionRecord::AutoResolved {
        rule_identity: "tool:read".to_string(),
        action: AutoResolution::Allow,
        reason: None,
        call_id: None,
        shows_preview: false,
        preview: None,
    });
    let deny = SessionRecord::Permission(PermissionRecord::AutoResolved {
        rule_identity: "global:*".to_string(),
        action: AutoResolution::Deny,
        reason: Some("Permission denied by rule 'global:*' (scope: global)".to_string()),
        call_id: None,
        shows_preview: false,
        preview: None,
    });

    // -- Exec
    let mut loud_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut loud_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            false,
        );
        renderer.draw(RenderInput::Record(&allow));
        renderer.draw(RenderInput::Record(&deny));
        renderer.flush();
    }
    let mut quiet_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut quiet_bytes,
            UiPolicy {
                quiet: true,
                verbosity: Verbosity::Quiet,
            },
            false,
        );
        renderer.draw(RenderInput::Record(&allow));
        renderer.draw(RenderInput::Record(&deny));
        renderer.flush();
    }

    // -- Check
    let loud = String::from_utf8(loud_bytes)?;
    assert!(
        loud.contains("permission auto-resolved: rule=tool:read action=allow"),
        "the allow line must name the rule and the action; got: {loud}"
    );
    assert!(
        !loud.contains("rule=tool:read action=allow reason="),
        "an allow line carries no reason; got: {loud}"
    );
    assert!(
        loud.contains(
            "permission auto-resolved: rule=global:* action=deny reason=Permission denied by rule 'global:*' (scope: global)"
        ),
        "the deny line must carry the reason; got: {loud}"
    );
    let quiet = String::from_utf8(quiet_bytes)?;
    assert!(
        quiet.is_empty(),
        "quiet mode must suppress the auto-resolved line; got: {quiet}"
    );
    Ok(())
}
