use super::*;

#[tokio::test]
async fn set_model_swaps_the_model_used_by_the_next_compact_call() -> Result<()> {
    let bus = Bus::default();
    let old_model = stream_model(&["old summary"]);
    let compactor = make_compactor(old_model, bus, None);

    // The new model is scripted to produce a distinct summary; keep a clone to
    // inspect the recorded request after the swap.
    let new_model = stream_model(&["new model summary"]);
    compactor.set_model(new_model.clone().erase());

    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact after set_model should succeed: {e:?}"))?;

    let msg: Message = artifact.into();
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}new model summary"),
        "compact must use the model set via set_model()"
    );
    let prompt = last_request_prompt(&new_model)?;
    assert!(
        prompt.contains("old message"),
        "the swapped-in model received the evicted messages, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn from_shared_model_compactor_uses_model_swapped_on_the_shared_arc() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    // The compactor is built from an external shared Arc; the old model is
    // scripted to produce a distinct summary that must NOT appear.
    let old_model = stream_model(&["old shared summary"]);
    let shared_arc: std::sync::Arc<std::sync::Mutex<DynModel<Completion>>> =
        std::sync::Arc::new(std::sync::Mutex::new(old_model.erase()));

    let compactor: NuCompactor<NoopStore> =
        NuCompactor::from_shared_model(shared_arc.clone(), bus, None);

    // The new model is scripted to produce a distinct summary; keep a clone to
    // inspect the recorded request after the swap.
    let new_model = stream_model(&["new shared summary"]);
    *shared_arc.lock().expect("shared model mutex not poisoned") = new_model.clone().erase();

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact after set_model on the shared Arc should succeed: {e:?}"))?;

    // -- Check
    let msg: Message = artifact.into();
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}new shared summary"),
        "compact must use the model swapped via the shared Arc"
    );
    let prompt = last_request_prompt(&new_model)?;
    assert!(
        prompt.contains("old message"),
        "the swapped-in model received the evicted messages, got: {prompt}"
    );
    Ok(())
}
