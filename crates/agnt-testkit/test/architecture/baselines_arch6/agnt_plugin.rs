//! The ARCH-6 rows recorded for `agnt-plugin`.

pub const ROWS: &[&str] = &[
    "ARCH-6 agnt-plugin/src/command/agent/args.rs:16: `extract_and_validate_session_flags` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/agent/args.rs:35: `extract_tools_from_call` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/agent/args.rs:93: `extract_tool_timeout` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/agent/command.rs:8: `Agent` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/agent/input.rs:3: `extract_prompt_from_input` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/agent/input.rs:58: `extract_context_from_input` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/agent/runtime_build.rs:30: `resolve_config` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/config/init.rs:7: `AgentConfigInit` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/mcp/auth/login.rs:38: `AgentAuthMcpLogin` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/mcp/auth/logout.rs:25: `AgentAuthMcpLogout` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/mcp/auth/status.rs:51: `AgentAuthMcpStatus` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/models/list.rs:7: `AgentModelsList` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/models/sync.rs:7: `AgentModelsSync` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/provider/auth/login.rs:8: `AgentProviderAuthLogin` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/provider/auth/logout.rs:9: `AgentProviderAuthLogout` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/provider/auth/status.rs:9: `AgentProviderAuthStatus` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/session/clear.rs:22: `AgentSessionClear` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/session/inspect.rs:24: `AgentSessionInspect` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/command/session/list.rs:22: `AgentSessionList` has no consumer outside `agnt-plugin`",
];

/// The impl-member rows: a `pub` member on a type this crate surfaces, with no
/// consumer outside it.
pub const MEMBER_ROWS: &[&str] = &[
    "ARCH-6 agnt-plugin/src/plugin.rs:33: `runtime` has no consumer outside `agnt-plugin`",
    "ARCH-6 agnt-plugin/src/plugin.rs:40: `resolve_store_type` has no consumer outside `agnt-plugin`",
];
