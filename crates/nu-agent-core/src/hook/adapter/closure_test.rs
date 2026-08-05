use super::*;

/// Compile-time check that ClosureToolAdapter implements Send + Sync.
///
/// This test ensures that our adapter can be safely shared across threads.
#[test]
fn closure_tool_adapter_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<ClosureToolAdapter>();
    assert_sync::<ClosureToolAdapter>();
}

/// Compile-time check that DynamicTool is Send + Sync.
///
/// This ensures that DynamicTool instances can be registered with rig's ToolServer.
#[test]
fn dynamic_tool_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<DynamicTool>();
    assert_sync::<DynamicTool>();
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
// 3. Verify the DynamicTool has the correct name
// 4. Verify the DynamicTool has a valid definition
// 5. Verify execution via ToolSet executes the closure and returns correct JSON
