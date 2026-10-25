//! Shared fixtures for the permission resolver test modules.
//!
//! Holds the imports and the resolver/config builders every topical
//! permission-resolver test file needs. Sub-files pull them in with
//! `use super::*;`.

pub(super) use std::collections::HashMap;
pub(super) use std::sync::{Arc, Mutex as StdMutex};

pub(super) use crate::bus::Bus;
pub(super) use crate::hook::permission_resolver::{
    AsyncPermissionResolver, InteractivePermissionResolver, PolicyPermissionResolver,
};
pub(super) use crate::tools::authz::{
    PermissionAction, PermissionDecision as AuthzPermissionDecision, PermissionRuleMatch,
    PermissionsConfig, SessionGrantCache,
};
pub(super) use crate::tools::closure::ClosureRegistry;
pub(super) use crate::tools::handler::McpToolRegistry;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a resolver pair for interactive tests: resolver + bus.
///
/// Uses the process cwd — fine for tests that pass absolute paths. Tests that
/// depend on relative-path resolution must use `make_interactive_with_cwd`.
pub(super) fn make_interactive(
    permissions: PermissionsConfig,
) -> (InteractivePermissionResolver, Bus) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    make_interactive_with_cwd(permissions, cwd)
}

/// Build a resolver pair for interactive tests with an explicit cwd.
pub(super) fn make_interactive_with_cwd(
    permissions: PermissionsConfig,
    cwd: std::path::PathBuf,
) -> (InteractivePermissionResolver, Bus) {
    make_interactive_with_cwd_and_previews(permissions, cwd, true)
}

/// Build a resolver pair for interactive tests, choosing whether the UI
/// renders pre-authorize previews. `renders_tool_previews: false` models the
/// ACP path, whose translator drops `ToolPreview`.
pub(super) fn make_interactive_with_cwd_and_previews(
    permissions: PermissionsConfig,
    cwd: std::path::PathBuf,
    renders_tool_previews: bool,
) -> (InteractivePermissionResolver, Bus) {
    let bus = Bus::default();
    let resolver = InteractivePermissionResolver {
        pending: Arc::new(StdMutex::new(HashMap::new())),
        permissions: Arc::new(permissions),
        session_grants: Arc::new(StdMutex::new(SessionGrantCache::default())),
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        cwd,
        previewed: Arc::new(StdMutex::new(std::collections::HashSet::new())),
        renders_tool_previews,
    };
    (resolver, bus)
}

pub(super) fn make_policy(permissions: PermissionsConfig) -> PolicyPermissionResolver {
    PolicyPermissionResolver {
        permissions: Arc::new(permissions),
        session_grants: Arc::new(StdMutex::new(SessionGrantCache::default())),
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
    }
}

/// `safe_defaults(false)` → global Deny.
/// All tools that don't match an explicit allow rule will be Deny.
pub(super) fn deny_all_config() -> PermissionsConfig {
    PermissionsConfig::safe_defaults(false)
}

/// `safe_defaults(true)` → global Ask, but "read" is explicitly Allow.
/// Use "read" as the "explicit allow" tool in tests.
pub(super) fn ask_global_with_read_allowed_config() -> PermissionsConfig {
    PermissionsConfig::safe_defaults(true)
}

/// A tool name that is not in the explicit allow list of safe_defaults(true)
/// so it falls through to the global Ask action.
pub(super) const ASK_TOOL: &str = "some_mcp_tool";

/// A tool that is explicitly allowed in `safe_defaults(true)`.
pub(super) const ALLOW_TOOL: &str = "read";

/// Global Ask, but `nu` is explicitly Allow — the auto-approve path for a
/// previewable tool (`nu` builds a code preview, `read` does not).
pub(super) fn nu_allowed_config() -> PermissionsConfig {
    PermissionsConfig::from_toml(
        &toml::from_str::<toml::Value>(
            r#"
"*" = "ask"
nu = "allow"
"#,
        )
        .expect("valid toml"),
        true,
    )
}

/// Global Deny with no tool rules — every tool is denied without a prompt.
pub(super) fn deny_all_toml_config() -> PermissionsConfig {
    PermissionsConfig::from_toml(
        &toml::from_str::<toml::Value>(
            r#"
"*" = "deny"
"#,
        )
        .expect("valid toml"),
        true,
    )
}
