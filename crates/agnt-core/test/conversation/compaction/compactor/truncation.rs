use super::*;

#[tokio::test]
async fn max_bytes_cap_truncates_body_at_utf8_boundary_preserving_header() -> Result<()> {
    let bus = Bus::default();
    // A summary long enough that the full artifact far exceeds the cap.
    let summary = "alpha beta gamma delta epsilon zeta eta theta";
    let model = stream_model(&[summary]);
    let compactor = make_compactor(model, bus, Some(40));

    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    let msg: Message = artifact.into();
    let text = user_text(&msg);
    let full = format!("{SUMMARY_HEADER}{summary}");

    assert!(
        full.len() > 40,
        "test precondition: full artifact must exceed the cap"
    );
    assert!(
        text.starts_with(SUMMARY_HEADER),
        "truncated artifact must keep the header, got: {text}"
    );
    assert!(
        text.ends_with(TRUNCATION_MARKER),
        "truncated artifact must end with the marker, got: {text}"
    );
    assert!(
        text.len() <= 40,
        "truncated artifact must not exceed max_bytes, got {text:?} ({} bytes)",
        text.len()
    );

    // The truncated body must be a UTF-8-boundary prefix of the full body,
    // so no grapheme is split mid-character.
    let body = text
        .strip_prefix(SUMMARY_HEADER)
        .and_then(|s| s.strip_suffix(TRUNCATION_MARKER))
        .ok_or("truncated text has header and marker")?;
    assert!(
        full.starts_with(&format!("{SUMMARY_HEADER}{body}")),
        "truncated body must be a prefix of the full body, got: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn max_bytes_cap_leaves_a_fitting_summary_untouched() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let summary = "short summary";
    let model = stream_model(&[summary]);
    let compactor = make_compactor(model, bus, Some(200));

    // -- Exec
    let artifact = compactor
        .compact(
            "conv-1",
            &[Message::user("old message")],
            None,
            "test_source",
        )
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    // -- Check
    let msg: Message = artifact.into();
    let text = user_text(&msg);
    assert_eq!(text, format!("{SUMMARY_HEADER}{summary}"));
    assert!(
        !text.contains(TRUNCATION_MARKER),
        "a summary that fits the cap must not be truncated, got: {text}"
    );
    Ok(())
}
