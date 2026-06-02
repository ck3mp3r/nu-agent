use super::*;
use crate::agent::hook::driver::PermissionResolver;
use crate::agent::tools::authz::{AskChoice, AutoApproveAskHook};
use crate::agent::tools::handler::{McpToolRegistry, ToolSource};
use crate::tools::closure::ClosureRegistry;
use nu_protocol::{BlockId, Span, Spanned, engine::Closure};

/// Mock ask hook that always denies
struct AlwaysDenyHook;

impl crate::agent::tools::authz::AskApprovalHook for AlwaysDenyHook {
    fn choose(
        &mut self,
        _decision: &crate::agent::tools::authz::PermissionDecision,
        _tool_name: &str,
        _args: &serde_json::Value,
    ) -> AskChoice {
        AskChoice::Deny
    }
}

/// Mock ask hook that always allows once
struct AlwaysAllowOnceHook;

impl crate::agent::tools::authz::AskApprovalHook for AlwaysAllowOnceHook {
    fn choose(
        &mut self,
        _decision: &crate::agent::tools::authz::PermissionDecision,
        _tool_name: &str,
        _args: &serde_json::Value,
    ) -> AskChoice {
        AskChoice::AllowOnce
    }
}

#[test]
fn resolver_type_checks_with_auto_approve_hook() {
    // Compile-time test: verify AuthzPermissionResolver type-checks correctly.
    // We don't instantiate or call methods because that would require a real EngineInterface.
    fn assert_type_is_permission_resolver<H>()
    where
        H: crate::agent::tools::authz::AskApprovalHook,
    {
        // This function asserts that AuthzPermissionResolver<H> implements PermissionResolver
        fn _assert_impl<T: PermissionResolver>() {}
        _assert_impl::<AuthzPermissionResolver<H>>();
    }

    assert_type_is_permission_resolver::<AutoApproveAskHook>();
}

#[test]
fn resolver_type_checks_with_custom_hooks() {
    // Verify AuthzPermissionResolver works with custom hook implementations
    fn assert_type_is_permission_resolver<H>()
    where
        H: crate::agent::tools::authz::AskApprovalHook,
    {
        fn _assert_impl<T: PermissionResolver>() {}
        _assert_impl::<AuthzPermissionResolver<H>>();
    }

    assert_type_is_permission_resolver::<AlwaysDenyHook>();
    assert_type_is_permission_resolver::<AlwaysAllowOnceHook>();
}

/// Helper to create a test closure for ClosureRegistry
fn create_test_closure() -> Spanned<Closure> {
    Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::unknown(),
    }
}

#[test]
fn resolve_tool_source_returns_closure_for_known_closure_tool() {
    let mut closure_registry = ClosureRegistry::new();
    closure_registry.register("run".to_string(), create_test_closure());

    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("run", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Closure);
}

#[test]
fn resolve_tool_source_returns_mcp_for_known_mcp_tool() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(["mcp_tool"]);

    let source = resolve_tool_source("mcp_tool", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Mcp);
}

#[test]
fn resolve_tool_source_returns_unknown_for_unregistered_tool() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("nonexistent", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Unknown);
}

#[test]
fn resolve_tool_source_prefers_closure_over_mcp() {
    let mut closure_registry = ClosureRegistry::new();
    closure_registry.register("ambiguous".to_string(), create_test_closure());

    let mcp_registry = McpToolRegistry::from_names(["ambiguous"]);

    let source = resolve_tool_source("ambiguous", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Closure);
}

#[test]
fn resolve_tool_source_finds_mcp_tool_by_namespaced_name() {
    use crate::tools::mcp::client::McpToolDefinition;

    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_tools(vec![McpToolDefinition {
        server: "nu".to_string(),
        name: "nu__run".to_string(),
        raw_name: "run".to_string(),
        description: None,
        parameters: None,
    }])
    .expect("registry should build");

    // Should find by namespaced name (what rig's tool server uses)
    let source = resolve_tool_source("nu__run", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Mcp);
}

#[test]
fn resolve_tool_source_returns_builtin_for_read() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("read", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Builtin);
}

#[test]
fn resolve_tool_source_returns_builtin_for_edit() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("edit", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::BuiltinFs);
}

#[test]
fn resolve_tool_source_returns_builtin_for_patch() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("patch", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::BuiltinFs);
}

#[test]
fn resolve_tool_source_returns_builtin_for_skill() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("skill", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Builtin);
}

#[test]
fn resolve_tool_source_prefers_closure_over_builtin() {
    let mut closure_registry = ClosureRegistry::new();
    closure_registry.register("read".to_string(), create_test_closure());

    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    // Closures should take precedence over builtins
    let source = resolve_tool_source("read", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Closure);
}

#[test]
fn resolve_tool_source_returns_builtin_for_send_message() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("send_message", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Builtin);
}

#[test]
fn resolve_tool_source_returns_builtin_for_list_agents() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("list_agents", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Builtin);
}

#[test]
fn resolve_tool_source_returns_builtin_for_spawn_agent() {
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);

    let source = resolve_tool_source("spawn_agent", &closure_registry, &mcp_registry);
    assert_eq!(source, ToolSource::Builtin);
}
