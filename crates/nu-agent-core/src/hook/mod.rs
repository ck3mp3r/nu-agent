pub mod adapter;
pub mod driver;
pub mod permission_bridge;
pub mod prompt_hook;
pub mod types;

pub use adapter::{BuiltinToolAdapter, adapt_builtins};
pub use adapter::{ClosureToolAdapter, adapt_closures};
pub use driver::{HookDriver, PermissionResolver};
pub use permission_bridge::{AuthzPermissionResolver, resolve_tool_source};
pub use prompt_hook::CopilotPromptHook;
pub use types::{HookEvent, PermissionDecision};
