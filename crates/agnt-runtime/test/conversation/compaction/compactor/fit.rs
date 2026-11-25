//! The summary requests fit their input to the model through rig's documented
//! consumer sequence: `serialize_conversation` -> `fit_to_model` ->
//! `summarization_request` with the fitted text and the fitter's output limit
//! (rig-memory-0.47.0/src/compaction/tests.rs:295-316).

use super::*;

use crate::conversation::compaction::compactor::resolve_model_spec;
use crate::conversation::compaction::summary;

/// A model with a 60k window and a 64k declared output limit: the regression's
/// shape (a huge declared output limit, no `limit.input` row), small enough for
/// a 20k-character fixture.
fn small_window_spec() -> Result<ModelSpec> {
    resolve_model_spec("openai", "fit-model", Some(60_000), Some(65_536), None)
        .ok_or_else(|| "openai should resolve to a registered provider id".into())
}

/// Seed `store` with a history whose serialized form far exceeds the fitted
/// budget, ending in a turn the cut splits.
fn seed_long_history(store: &dyn Store) -> Result<()> {
    let mut conversation = rig_memory::load(store, CONV)?;
    let filler = "x".repeat(500);
    let mut messages = vec![Message::user(format!("OLDESTMARK {filler}"))];
    for at in 1..36 {
        messages.push(if at % 2 == 0 {
            Message::user(format!("question {at} {filler}"))
        } else {
            Message::assistant(format!("answer {at} {filler}"))
        });
    }
    // The summarized slice's tail: the marker proves the end is kept.
    messages.push(Message::assistant(format!("TAILMARK {filler}")));
    // The split turn's earlier part, then a reply large enough that the cut
    // keeps from it.
    messages.push(Message::user("SPLITTURN the newest question"));
    messages.push(Message::assistant(format!("the newest answer {filler}")));
    conversation.extend(messages);
    rig_memory::save(store, CONV, &mut conversation)?;
    Ok(())
}

#[tokio::test]
async fn compact_fits_the_history_request_to_the_models_input_budget() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store().await?;
    seed_long_history(&*store)?;
    let bus = Bus::default();
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text("fitted summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("prefix summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let compactor = NuCompactor::new(
        model.clone().erase(),
        bus,
        test_settings(),
        Some(small_window_spec()?),
    );

    // -- Exec
    compactor
        .compact(Arc::clone(&store), CONV, false, CompactionSource::Auto)
        .await?;

    // -- Check: the history request carries the fitted end of the slice and the
    // fitter's output limit (the derived budget is 60_000 - 16_384 - 32_768, and
    // `fit_to_model` splits its room: 5_424 / 2 = 2_712 at most).
    let requests = model.requests();
    assert_eq!(requests.len(), 2, "history and split-turn requests");
    let history = requests.first().ok_or("the history request is recorded")?;
    let history_prompt = request_prompt(history)?;
    assert_eq!(
        history.max_tokens,
        Some(2_712),
        "the request asks for the fitter's output limit"
    );
    assert!(
        history_prompt.contains("earlier characters left out"),
        "the fitted prompt names the characters it left out"
    );
    assert!(
        !history_prompt.contains("OLDESTMARK"),
        "the fitted prompt must drop the head of the slice"
    );
    assert!(
        history_prompt.contains("TAILMARK"),
        "the fitted prompt must keep the end of the slice"
    );

    // -- Check: the split turn's earlier part follows the same sequence.
    let prefix = requests
        .get(1)
        .ok_or("the split-turn request is recorded")?;
    let prefix_prompt = request_prompt(prefix)?;
    assert_eq!(
        prefix.max_tokens,
        Some(2_712),
        "the split-turn request asks for the fitter's output limit"
    );
    assert!(
        prefix_prompt.contains("# Conversation\n[User]: SPLITTURN"),
        "the split-turn prompt carries the fitted serialization, got: {prefix_prompt}"
    );
    Ok(())
}

#[test]
fn test_conversation_compaction_fit_derives_the_input_limit_from_the_window() -> Result<()> {
    // -- Setup & Fixtures: a model that declares no `limit.input` row.
    let spec = small_window_spec()?;

    // -- Exec
    let limit = summary::effective_input_limit(&spec, test_settings())?;

    // -- Check: the window less the reply reserve and the summary budget.
    assert_eq!(limit, 10_848, "60_000 - 16_384 reserve - 32_768 summary");
    Ok(())
}

#[test]
fn test_conversation_compaction_fit_a_window_under_the_reserve_is_its_own_limit() -> Result<()> {
    // -- Setup & Fixtures: a window smaller than the reserve and the summary
    // budget, as a 5k model is.
    let spec = resolve_model_spec("openai", "tiny", Some(5_000), Some(1_024), None)
        .ok_or("openai should resolve to a registered provider id")?;

    // -- Exec
    let limit = summary::effective_input_limit(&spec, test_settings())?;

    // -- Check: nothing to give up, so the window itself bounds the request.
    assert_eq!(
        limit, 5_000,
        "a window with no room left below the reserve is its own limit"
    );
    Ok(())
}

#[test]
fn test_conversation_compaction_fit_unknown_window_fails_named() -> Result<()> {
    // -- Setup & Fixtures: a spec that knows neither an input limit nor a window.
    let spec = ModelSpec::new(provider_id()?, "no-window");

    // -- Exec
    let request = summary::history_request(
        &[Message::user("only message")],
        None,
        "",
        &spec,
        test_settings(),
    );

    // -- Check
    let Err(err) = request else {
        panic!("a spec with no window must not build a request");
    };
    assert!(
        err.to_string().contains("no known context window"),
        "the failure names the unknown window, got: {err}"
    );
    Ok(())
}

#[test]
fn test_conversation_compaction_fit_bounds_the_prompt_without_an_input_row() -> Result<()> {
    // -- Setup & Fixtures: a slice far over the derived budget.
    let spec = small_window_spec()?;
    let filler = "x".repeat(20_000);
    let messages = [
        Message::user(format!("OLDESTMARK {filler}")),
        Message::assistant("TAILMARK answer"),
    ];

    // -- Exec
    let request = summary::history_request(&messages, None, "", &spec, test_settings())?;

    // -- Check: the None-limit branch allows 400_000 characters; the derived one
    // allows (5_424 - 2_712) * 3.
    let prompt = request_prompt(&request)?;
    assert!(
        !prompt.contains("OLDESTMARK"),
        "the prompt must drop the head of the slice"
    );
    assert!(
        prompt.contains("TAILMARK"),
        "the prompt must keep the end of the slice"
    );
    assert!(
        prompt.contains("earlier characters left out"),
        "the bounded fit names the characters it left out"
    );
    Ok(())
}

#[test]
fn test_conversation_compaction_fit_matches_rigs_own_request_when_the_slice_fits() -> Result<()> {
    // -- Setup & Fixtures: a slice well inside the model's budget.
    let spec = test_spec()?;
    let messages = [
        Message::user("first question"),
        Message::assistant("first answer"),
    ];

    // -- Exec
    let ours = summary::history_request(&messages, None, "", &spec, test_settings())?;
    let rigs = rig_memory::summary_request(&messages, (None, ""), &spec, None)
        .map_err(|e| format!("rig's builder must build: {e}"))?;

    // -- Check: the same bytes as rig's own builder when nothing needs fitting.
    assert_eq!(
        request_prompt(&ours)?,
        request_prompt(&rigs)?,
        "a slice that fits must go out exactly as rig frames it"
    );
    assert_eq!(
        ours.max_tokens, rigs.max_tokens,
        "the output limit is the one rig asks for"
    );
    Ok(())
}

#[tokio::test]
async fn compact_fails_named_when_the_models_window_is_unknown() -> Result<()> {
    // -- Setup & Fixtures: a compactor whose spec knows no window.
    let store = memory_store().await?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.stream().subscribe();
    let compactor = NuCompactor::new(
        stream_model(&["unused summary"]).erase(),
        bus,
        test_settings(),
        Some(ModelSpec::new(provider_id()?, "no-window")),
    );

    // -- Exec
    let result = compactor
        .compact(Arc::clone(&store), CONV, false, CompactionSource::Auto)
        .await;

    // -- Check: bounded or failed, never unbounded.
    let Err(MemoryError::Backend(err)) = result else {
        panic!("an unbounded summary request must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().contains("no known context window"),
        "the error names the unknown window, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("no known context window"),
        "expected Failed naming the unknown window, got {failed:?}"
    );
    Ok(())
}

// region:    --- Test Support

/// A registered provider id, for a spec built without the config resolution.
fn provider_id() -> Result<rig::providers::registry::ProviderId> {
    rig::providers::registry::ProviderId::new("openai", rig::providers::registry::Format::OpenAi)
        .ok_or_else(|| "openai should resolve to a registered provider id".into())
}

// endregion: --- Test Support
