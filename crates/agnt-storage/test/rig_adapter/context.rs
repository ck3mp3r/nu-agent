use super::*;

/// rig's `Store` is synchronous, and its callers run inside the host's async
/// paths: the adapter answers from an asynchronous context.
#[tokio::test]
async fn test_rig_adapter_answers_from_an_async_context() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-async")?;

    // -- Exec & Check
    store.append_log("agent", 0, b"one")?;
    store.write_file("name", b"a name")?;
    assert_eq!(store.read_log("agent")?, b"one");
    assert_eq!(store.read_file("name")?, b"a name");
    Ok(())
}

/// The same, from a task the host spawned on its runtime.
#[tokio::test]
async fn test_rig_adapter_answers_from_a_spawned_task() -> Result<()> {
    // -- Setup & Fixtures
    let store = Arc::new(memory_store("session-async-task")?);

    // -- Exec
    let worker = store.clone();
    let written = tokio::spawn(async move {
        worker.append_log("agent", 0, b"one")?;
        worker.read_log("agent")
    })
    .await?;

    // -- Check
    assert_eq!(written?, b"one");
    Ok(())
}
