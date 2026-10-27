//! Token accounting: what the completion reports and what the hook stores.

use super::super::*;
use super::*;

/// A usage that reports both counters but no total emits `total_tokens: None`,
/// so the TUI can leave its displayed count alone instead of collapsing it to
/// zero.
#[tokio::test]
async fn total_less_usage_emits_no_total_tokens() -> Result<()> {
    // -- Setup & Fixtures: the ollama-cloud shape — counters present, total
    // absent.
    let model = catalog_turn(
        "copilot/claude-sonnet-4.6",
        Usage::new().input_tokens(1_000).output_tokens(500),
    )?;

    // -- Exec
    let total_tokens = emitted_total_tokens(completed_event(model).await?)?;

    // -- Check
    assert_eq!(
        total_tokens, None,
        "an absent total must reach the payload as None, not zero"
    );
    Ok(())
}

/// A usage that reports input and output but no total still stores the real
/// count: the switch gate and the threshold read `input + output`.
#[tokio::test]
async fn total_tokens_stored_from_the_counters_when_the_usage_reports_no_total() -> Result<()> {
    // -- Setup & Fixtures: the ollama-cloud shape — counters present, total
    // absent.
    let model = catalog_turn(
        "copilot/claude-sonnet-4.6",
        Usage::new().input_tokens(1_000).output_tokens(500),
    )?;
    let TurnHook {
        hook,
        bus,
        last_total_tokens,
        ..
    } = turn_hook(&model, pricing_less_summarizer())?;

    // -- Exec
    drive_turn(hook, &bus, model).await?;

    // -- Check
    let stored = *last_total_tokens.lock().expect("model mutex poisoned");
    assert_eq!(
        stored,
        Some(1_500),
        "a total-less usage must store input + output"
    );
    Ok(())
}

/// A usage that reports no counter at all emits nothing: the turn had no
/// measurable reply, so there is nothing to price or count.
#[tokio::test]
async fn no_event_when_the_usage_reports_no_counter() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_turn("copilot/claude-sonnet-4.6", Usage::default())?;

    // -- Exec
    let event = emitted_completion(model).await?;

    // -- Check
    assert!(
        event.is_none(),
        "a usage that reports no counter must emit nothing"
    );
    Ok(())
}
