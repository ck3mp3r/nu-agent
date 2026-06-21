pub mod adapter;
pub mod agent_hook;
pub mod permission_bridge;
pub mod permission_resolver;

pub use adapter::{BuiltinToolAdapter, ClosureToolAdapter, adapt_builtins, adapt_closures};
pub use agent_hook::AgentHook;
pub use permission_bridge::resolve_tool_source;
pub use permission_resolver::{
    AsyncPermissionResolver,
    InteractivePermissionResolver,
    PolicyPermissionResolver,
    PermissionDecision,
};
