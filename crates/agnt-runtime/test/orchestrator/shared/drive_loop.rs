use super::*;

/// Drive the loop to completion and hand the runtime back.
///
/// The loop mints its own span, so the `span` argument only keeps the fixtures'
/// call shape; it is not read. The runtime is `None` only when the worker task
/// failed, which the fixtures never do.
pub(crate) async fn drive_loop<R, F, H>(
    runtime: R,
    config: LoopConfig<F, H>,
    _span: Span,
) -> (Option<R>, Result<Value, LabeledError>)
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + crate::catalog::PickerCatalogSource
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
    H: crate::orchestrator::AgentSwitchHook + Clone + Send + 'static,
{
    Loop::compose(runtime, config).run().await
}

/// The runtime a finished loop handed back, for the fixtures that read it.
pub(crate) fn handed_back<R>(runtime: Option<R>) -> Result<R, Box<dyn std::error::Error>> {
    runtime.ok_or_else(|| "the loop must hand its runtime back".into())
}
