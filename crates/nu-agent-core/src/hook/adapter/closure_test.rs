use super::*;
use rig::tool::ToolDyn;

/// Compile-time check that ClosureToolAdapter implements Send + Sync.
///
/// This test ensures that our adapter can be safely shared across threads,
/// which is required by rig's ToolDyn trait.
#[test]
fn closure_tool_adapter_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<ClosureToolAdapter>();
    assert_sync::<ClosureToolAdapter>();
}

/// Compile-time check that ToolDyn trait object is Send + Sync.
///
/// This ensures that boxed trait objects can be registered with rig's ToolServer.
#[test]
fn tool_dyn_trait_object_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<Box<dyn ToolDyn>>();
    assert_sync::<Box<dyn ToolDyn>>();
}

// Note: More comprehensive tests for `name()`, `definition()`, and `call()`
// would require setting up a full Nushell engine environment with:
// - A running nu_plugin EngineInterface
// - A ToolExecutor with audit logger
// - Valid Spanned<Closure> instances
//
// These tests would be complex integration tests. For now, we verify the critical
// trait bounds (Send + Sync) and rely on manual testing with the full engine.
//
// TODO: Add integration tests that:
// 1. Create a mock EngineInterface
// 2. Build a simple closure (e.g., {|x| $x + 1})
// 3. Verify name() returns the correct tool name
// 4. Verify definition() returns a valid ToolDefinition
// 5. Verify call() executes the closure and returns correct JSON
