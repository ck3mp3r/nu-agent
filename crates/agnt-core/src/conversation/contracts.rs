use crate::conversation::runtime::PendingPermissions;

/// The composition-time choices a surface makes about its runtime.
///
/// Both are settled before the first turn and never change during a run: which
/// decision source the permission gate asks, and whether the surface draws
/// pre-authorize previews. They live on a trait so a surface's mode functions
/// can configure the runtime without naming the aggregate it was composed into.
pub trait DecisionSource {
    fn with_decision_source(self, resolver: Option<PendingPermissions>) -> Self;
    fn with_tool_previews(self, renders: bool) -> Self;
}

/// The runtime facts a surface reads before it drives a turn.
///
/// Separate from [`CoreRuntime`](agnt_runtime::protocol::contracts::CoreRuntime)
/// on purpose: the loop's doubles implement a turn and nothing else, and a
/// supertrait here would make every one of them stub seven capabilities it
/// never calls (ARCH-7).
pub trait RuntimeAccess {
    /// The resolved application paths.
    fn paths(&self) -> &agnt_runtime::config::Paths;

    /// The agent snapshot every hook envelope is stamped with.
    fn hook_context(&self) -> &agnt_runtime::hooks::HookAgentContext;

    /// The shared cancellation bus, cloned for a turn or a surface.
    fn bus_handle(&self) -> agnt_runtime::bus::Bus;

    /// The provider serving this runtime.
    fn provider_name(&self) -> &str;

    /// The model serving this runtime.
    fn model(&self) -> &str;

    /// The session working directory the MCP servers were configured against.
    fn mcp_caller_cwd(&self) -> Option<&std::path::Path>;

    /// The MCP servers' lifecycle projection.
    fn mcp_lifecycle_projection(&self) -> &[agnt_runtime::tools::mcp::runtime::McpServerLifecycle];

    /// The personas an agent switch can pick from.
    fn available_agent_summaries(&self) -> &[agnt_runtime::protocol::persona::PersonaSummary];

    /// The active agent's identity line, when one is configured.
    fn agent_identity(&self) -> Option<&str>;

    /// The active agent's icon, when one is configured.
    fn agent_icon(&self) -> Option<&str>;

    /// The plugin configuration this runtime started from.
    fn startup_plugin_config(&self) -> Option<&agnt_runtime::config::PluginConfig>;

    /// Spawn a future on the runtime's tokio runtime.
    ///
    /// The member exists so a mode that drives a turn through these seams never
    /// names the aggregate to reach the tokio handle it owns. Its production
    /// callers are the two `run_stderr_turn` drain spawns; the aggregate's impl
    /// is one delegate to the inherent twin.
    fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static;
}
