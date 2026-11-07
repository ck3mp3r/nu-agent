use super::*;

/// Drive the loop to completion and hand the runtime back.
///
/// The loop mints its own span, so the `span` argument only keeps the fixtures'
/// call shape; it is not read.
pub(crate) async fn drive_loop<R, F, H>(
    runtime: R,
    config: LoopConfig<F, H>,
    _span: Span,
) -> (R, Result<Value, LabeledError>)
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
    H: crate::orchestrator::AgentSwitchHook + Clone + Send + 'static,
{
    Loop::compose(runtime, config).run().await
}
