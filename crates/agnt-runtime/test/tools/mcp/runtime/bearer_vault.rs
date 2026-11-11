use super::*;

#[test]
fn http_bearer_resolution_table() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let vault = vault_with_bearer(&dir, "my-mcp-token", "actual-secret")?;
    let empty = empty_vault(&dir)?;

    let cases: Vec<(&str, &str, Option<&Vault>, &str)> = vec![
        (
            "store ref resolves from vault",
            "store:my-mcp-token",
            Some(&vault),
            "actual-secret",
        ),
        (
            "raw token is used without vault lookup",
            "raw-token-value",
            Some(&empty),
            "raw-token-value",
        ),
        (
            "store ref without vault uses the raw reference",
            "store:my-mcp-token",
            None,
            "store:my-mcp-token",
        ),
    ];

    // -- Exec & Check
    for (name, token, vault, expected) in cases {
        let server = bearer_server(token);
        let config = build_http_transport_config(&server, vault).map_err(|e| e.to_string())?;
        assert_eq!(
            config.auth_header.as_deref(),
            Some(expected),
            "case '{name}': auth header"
        );
    }
    Ok(())
}

#[test]
fn http_bearer_missing_store_ref_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let vault = empty_vault(&dir)?;
    let server = bearer_server("store:missing");

    // -- Exec
    let config = build_http_transport_config(&server, Some(&vault));

    // -- Check
    let message = config.err().ok_or("should fail for a missing token")?;
    assert!(
        message.contains("failed to resolve bearer token"),
        "error names the bearer resolution failure, got: {message}"
    );
    assert!(
        message.contains("mcp:bearer:missing"),
        "error names the missing vault key, got: {message}"
    );
    Ok(())
}
