use std::path::Path;

/// Resolve the mesh key with precedence (highest wins):
/// 1. Explicit value passed directly
/// 2. `AGENT_MESH_KEY` env var
/// 3. Default: SHA-256 of `engine_cwd`, first 7 hex chars
pub fn resolve_mesh_key(explicit: Option<String>, engine_cwd: &Path) -> String {
    if let Some(key) = explicit {
        return key;
    }
    if let Ok(env_key) = std::env::var("AGENT_MESH_KEY")
        && !env_key.is_empty()
    {
        return env_key;
    }
    default_mesh_key(engine_cwd)
}

/// SHA-256 of `cwd`, first 7 hex chars.
pub fn default_mesh_key(cwd: &Path) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(cwd.to_string_lossy().as_bytes());
    hex::encode(&hash[..])[..7].to_string()
}
