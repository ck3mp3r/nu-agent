use super::*;

#[tokio::test]
async fn set_model_swaps_the_model_used_by_the_next_compact_call() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store().await?;
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
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
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

/// WHEN the runtime switches models between compactions, THEN THE compactor's slot
/// SHALL hold the new active model at its next call: the call already in flight
/// keeps the model it took, and the next one summarizes on the switched model.
#[tokio::test]
async fn the_slot_holds_the_switched_model_at_the_next_compaction_call() -> Result<()> {
    // -- Setup & Fixtures: a compactor over the shared slot the runtime switches,
    // scripted to answer "old summary" first.
    let store = memory_store().await?;
    seed(&*store)?;
    let bus = Bus::default();
    let old_model = stream_model(&["old summary"]);
    let shared_arc: Arc<std::sync::Mutex<DynModel<Completion>>> =
        Arc::new(std::sync::Mutex::new(old_model.erase()));
    let compactor = NuCompactor::from_shared_model(
        Arc::clone(&shared_arc),
        bus,
        test_policy(),
        Some(test_spec()?),
    );

    // -- Exec: compact once, then switch the shared slot and grow the
    // conversation for a second compaction.
    let first = compactor
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
        .await?
        .ok_or("the first compaction should have run")?;
    seed(&*store)?;
    let new_model = stream_model(&["new model summary"]);
    *shared_arc
        .lock()
        .map_err(|_| "shared model mutex not poisoned")? = new_model.clone().erase();
    let second = compactor
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
        .await?
        .ok_or("the second compaction should have run")?;

    // -- Check: the first call kept the model it took; the second, after the
    // switch, used the new one.
    let first_summary = first
        .messages
        .first()
        .ok_or("the spliced conversation is non-empty")?;
    assert!(
        user_text(first_summary).contains("old summary"),
        "the call before the switch summarizes on the model it took, got: {}",
        user_text(first_summary)
    );
    let second_summary = second
        .messages
        .first()
        .ok_or("the spliced conversation is non-empty")?;
    assert!(
        user_text(second_summary).contains("new model summary"),
        "the next call after the switch summarizes on the new model, got: {}",
        user_text(second_summary)
    );
    Ok(())
}

#[tokio::test]
async fn from_shared_model_compactor_uses_model_swapped_on_the_shared_arc() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store().await?;
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
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
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
