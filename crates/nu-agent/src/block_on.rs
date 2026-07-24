/// Bridges a sync plugin command `run()` method to an async future.
///
/// Two forms:
///
/// 1. `block_on!(plugin, future)` — uses the shared runtime stored on the plugin
///    (preferred for commands that receive an `AgentPlugin` reference).
///
/// 2. `block_on!(future)` — creates a new multi-threaded tokio runtime
///    (fallback for contexts without a shared runtime, e.g. `CardFetcher`).
///
/// This is the only place `block_on` should appear — at the sync/async boundary
/// of the nu-plugin API.
///
/// # Examples
///
/// ```ignore
/// // Using shared plugin runtime (preferred)
/// fn run(&self, plugin: &AgentPlugin, engine: &EngineInterface, call: &EvaluatedCall, _input: &Value) -> Result<Value, LabeledError> {
///     block_on!(plugin, self.run_inner(engine, call))
/// }
///
/// // Creating a new runtime (fallback)
/// fn run(&self, _plugin: &AgentPlugin, engine: &EngineInterface, call: &EvaluatedCall, _input: &Value) -> Result<Value, LabeledError> {
///     block_on!(async { do_something().await })
/// }
/// ```
#[macro_export]
macro_rules! block_on {
    ($plugin:expr, $future:expr) => {{ $plugin.runtime.block_on($future) }};
    ($future:expr) => {{
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                nu_protocol::LabeledError::new(format!("Failed to create runtime: {e}"))
            })?;
        rt.block_on($future)
    }};
}
