use super::*;

#[test]
fn permission_request_cancelled_renders_line_and_quiet_suppresses()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let record = SessionRecord::Permission(PermissionRecord::Cancelled {
        request_id: RequestId::from_minted("perm-0000000000000001"),
        rule_identity: "global:*".to_string(),
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
        draw(&mut renderer, record.clone());
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
        draw(&mut renderer, record.clone());
        renderer.flush();
    }

    // -- Check
    let loud = String::from_utf8(loud_bytes)?;
    assert!(
        loud.contains(
            "permission request cancelled: request_id=perm-0000000000000001 rule=global:*"
        ),
        "the cancelled line must name the request and the rule; got: {loud}"
    );
    let quiet = String::from_utf8(quiet_bytes)?;
    assert!(
        quiet.is_empty(),
        "quiet mode must suppress the cancelled line; got: {quiet}"
    );
    Ok(())
}
