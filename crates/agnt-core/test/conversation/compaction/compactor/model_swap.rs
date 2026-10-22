use super::*;

#[tokio::test]
async fn set_model_swaps_the_model_used_by_the_next_compact_call() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store()?;
    seed(&*store)?;
    let bus = Bus::default();
    let old_model = stream_model(&["old summary"]);
    let compactor = NuCompactor::new(old_model.erase(), bus, test_policy(), Some(test_spec()?));

    // The new model is scripted to produce a distinct summary; keep a clone to
    // inspect the recorded request after the swap.
    let new_model = stream_model(&["new model summary"]);
    compactor.set_model(new_model.clone().erase());

    // -- Exec
    let outcome = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "test_source")
        .await?
        .ok_or("compaction should have run")?;

    // -- Check
    let summary = outcome
        .messages
        .first()
        .ok_or("the spliced conversation is non-empty")?;
    assert!(
        user_text(summary).contains("new model summary"),
        "compact must use the model set via set_model(), got: {}",
        user_text(summary)
    );
    let prompt = last_request_prompt(&new_model)?;
    assert!(
        prompt.contains("first question"),
        "the swapped-in model received the summarized messages, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn from_shared_model_compactor_uses_model_swapped_on_the_shared_arc() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store()?;
    seed(&*store)?;
    let bus = Bus::default();
    // The compactor is built from an external shared Arc; the old model is
    // scripted to produce a distinct summary that must NOT appear.
    let old_model = stream_model(&["old shared summary"]);
    let shared_arc: Arc<std::sync::Mutex<DynModel<Completion>>> =
        Arc::new(std::sync::Mutex::new(old_model.erase()));
    let compactor = NuCompactor::from_shared_model(
        Arc::clone(&shared_arc),
        bus,
        test_policy(),
        Some(test_spec()?),
    );

    // The new model is scripted to produce a distinct summary; keep a clone to
    // inspect the recorded request after the swap.
    let new_model = stream_model(&["new shared summary"]);
    *shared_arc
        .lock()
        .map_err(|_| "shared model mutex not poisoned")? = new_model.clone().erase();

    // -- Exec
    let outcome = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "test_source")
        .await?
        .ok_or("compaction should have run")?;

    // -- Check
    let summary = outcome
        .messages
        .first()
        .ok_or("the spliced conversation is non-empty")?;
    assert!(
        user_text(summary).contains("new shared summary"),
        "compact must use the model swapped via the shared Arc, got: {}",
        user_text(summary)
    );
    let prompt = last_request_prompt(&new_model)?;
    assert!(
        prompt.contains("first question"),
        "the swapped-in model received the summarized messages, got: {prompt}"
    );
    Ok(())
}
