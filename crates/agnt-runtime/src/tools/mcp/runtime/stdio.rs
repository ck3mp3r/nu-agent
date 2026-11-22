//! The stdio arm's path and environment resolution: the caller's cwd, the
//! per-server cwd override, and the env the child is given.

pub(crate) fn resolve_stdio_cwd(
    caller_cwd: &std::path::Path,
    cwd_override: Option<String>,
    server_name: &str,
) -> Result<std::path::PathBuf, String> {
    let canonical_caller = std::fs::canonicalize(caller_cwd).map_err(|e| {
        format!(
            "invalid caller cwd for stdio MCP server '{}': {} ({e})",
            server_name,
            caller_cwd.display()
        )
    })?;

    if !canonical_caller.is_dir() {
        return Err(format!(
            "invalid caller cwd for stdio MCP server '{}': '{}' is not a directory",
            server_name,
            canonical_caller.display()
        ));
    }

    if let Some(override_cwd) = cwd_override {
        let trimmed = override_cwd.trim();
        if trimmed.is_empty() {
            return Err(format!(
                "invalid stdio cwd override for MCP server '{}': path is empty",
                server_name
            ));
        }

        let path = std::path::PathBuf::from(trimmed);
        let effective_path = if path.is_absolute() {
            path.clone()
        } else {
            canonical_caller.join(&path)
        };

        let canonical = std::fs::canonicalize(&effective_path).map_err(|e| {
            format!(
                "invalid stdio cwd override for MCP server '{}': {} ({e})",
                server_name,
                effective_path.display()
            )
        })?;

        if !canonical.is_dir() {
            return Err(format!(
                "invalid stdio cwd override for MCP server '{}': '{}' is not a directory",
                server_name,
                canonical.display()
            ));
        }

        return Ok(canonical);
    }

    Ok(canonical_caller)
}

pub(crate) fn resolve_caller_cwd(
    caller_cwd: Option<&std::path::Path>,
    server_name: &str,
) -> Result<std::path::PathBuf, String> {
    let caller = caller_cwd.ok_or_else(|| {
        format!(
            "missing caller cwd for stdio MCP server '{}': provide invocation cwd",
            server_name
        )
    })?;

    resolve_stdio_cwd(caller, None, server_name)
}

pub(crate) fn merged_stdio_env_with_pwd(
    mut env: std::collections::HashMap<String, String>,
    effective_cwd: &std::path::Path,
    caller_cwd: &std::path::Path,
) -> std::collections::HashMap<String, String> {
    env.insert(
        "PWD".to_string(),
        effective_cwd.to_string_lossy().to_string(),
    );
    env.insert(
        "NU_AGENT_CALLER_CWD".to_string(),
        caller_cwd.to_string_lossy().to_string(),
    );
    env.insert(
        "NU_AGENT_CALLER_PWD".to_string(),
        caller_cwd.to_string_lossy().to_string(),
    );
    env
}
