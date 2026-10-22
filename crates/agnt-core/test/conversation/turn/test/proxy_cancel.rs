use super::*;

// ---------------------------------------------------------------------------
// FilteredToolProxy cancellation tests
// ---------------------------------------------------------------------------

/// Publishing a cancel event while a tool is executing causes FilteredToolProxy
/// to return a Cancelled error immediately, without waiting for the (potentially
/// hanging) tool body to finish.
///
/// The cancel must be published AFTER the tool starts running: the proxy subscribes
/// to the bus cancel channel inside its execution closure, and broadcast channels
/// only deliver messages sent after the receiver subscribes. The tool body signals
/// its start, so the cancel is published only once the subscription is live.
#[tokio::test]
async fn filtered_tool_proxy_cancels_during_execution() -> Result<()> {
    let handle = rig::tool::server::ToolServer::new().run();
    // Register a tool that never completes on its own, so only the cancel path
    // can end the call. The body signals its start before pending forever.
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let started_for_tool = std::sync::Arc::clone(&started);
    handle.add_dynamic_tool(rig::tool::DynamicTool::new(
        ToolName::new("sleeping_tool")?,
        "runs until cancelled",
        serde_json::json!({}),
        move |_args| {
            let started = std::sync::Arc::clone(&started_for_tool);
            Box::pin(async move {
                started.notify_one();
                std::future::pending::<()>().await;
                Ok(rig::tool::ToolOutput::text("done"))
            })
        },
    ));

    let bus = crate::bus::create_bus();
    let proxy = FilteredToolProxy {
        tool_name: ToolName::new("sleeping_tool")?,
        tool_definition: ToolDefinition {
            annotations: crate::types::ToolAnnotations::default(),
            name: ToolName::new("sleeping_tool")?,
            description: "test".to_string(),
            parameters: serde_json::json!({}),
        },
        handle,
        bus: bus.clone(),
    };

    // Convert to DynamicTool and execute via ToolSet.
    let dynamic_tool = proxy.into_dynamic_tool();
    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    // Publish the cancel once the tool body has started: the proxy subscribes
    // to the cancel channel before dispatching, so the subscription is live.
    let bus2 = bus.clone();
    let cancel_handle = tokio::spawn(async move {
        started.notified().await;
        let _ = bus2.cancel().send(crate::bus::CancelEvent::Requested).await;
    });

    let mut context = rig::tool::ToolContext::new();
    let result = toolset.execute("sleeping_tool", "{}", &mut context).await;
    cancel_handle
        .await
        .map_err(|e| format!("cancel task panicked: {e:?}"))?;

    assert!(
        result.is_error_kind(rig::tool::ToolErrorKind::Cancelled),
        "A cancel during execution must produce a Cancelled error"
    );
    Ok(())
}
