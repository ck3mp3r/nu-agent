use nu_protocol::{LabeledError, Span, Value};

use super::contracts::{ConversationRuntime, InteractiveUi, ProgressUi, UiMessageSnapshot};

fn is_llm_cancellation_error(error: &LabeledError) -> bool {
    error.msg == "LLM call cancelled"
}

pub(crate) fn run_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime,
    U: InteractiveUi,
{
    loop {
        ui.pump_once();

        if let Some(error) = ui.fatal_error() {
            return Err(LabeledError::new(format!("Interactive UI failed: {error}")));
        }

        while let Some(prompt) = ui.take_submitted_prompt() {
            if prompt.trim().is_empty() {
                continue;
            }

            // Interactive TUI mode is conversation-driven: per-turn results are rendered
            // in-pane and must not be emitted back to Nushell stdout.
            match runtime.execute_turn(ui, prompt, None, span) {
                Ok(_) => {}
                Err(error) if is_llm_cancellation_error(&error) => {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }

        if ui.quit_requested() {
            break;
        }
    }

    Ok(Value::nothing(span))
}

pub(crate) fn run_hydrated_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime,
    U: InteractiveUi,
{
    ui.hydrate_transcript_from_messages(messages);
    run_interactive_loop(runtime, ui, span)
}

pub(crate) fn run_single_turn<R, U>(
    runtime: &mut R,
    ui: &mut U,
    prompt: String,
    context: Option<String>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime,
    U: ProgressUi,
{
    runtime.execute_turn(ui, prompt, context, span)
}
