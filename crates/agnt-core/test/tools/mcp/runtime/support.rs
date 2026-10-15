use super::*;

// region:    --- Test Support

/// A `McpServerConfig` using HTTP transport and a Bearer `token`.
pub(super) fn bearer_server(token: &str) -> McpServerConfig {
    McpServerConfig {
        name: "bearer-test".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://api.example.com/mcp".to_string()),
        headers: HashMap::new(),
        auth: McpAuthConfig::Bearer {
            token: token.to_string(),
        },
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    }
}

/// A `FileBackend`-backed vault with `mcp:bearer:<name>` set to `secret`.
pub(super) fn vault_with_bearer(
    dir: &tempfile::TempDir,
    name: &str,
    secret: &str,
) -> Result<Vault> {
    use crate::config::vault::VaultBackend;

    let vault = empty_vault(dir)?;
    vault
        .backend()
        .set(&format!("mcp:bearer:{name}"), secret)
        .map_err(|e| e.to_string())?;
    Ok(vault)
}

/// A `FileBackend`-backed vault inside `dir`.
pub(super) fn empty_vault(dir: &tempfile::TempDir) -> Result<Vault> {
    let backend = crate::config::file_backend::FileBackend::new(dir.path().join("secrets.json"));
    Ok(Vault::new(crate::config::vault::VaultBackendKind::File(
        backend,
    )))
}

// endregion: --- Test Support
