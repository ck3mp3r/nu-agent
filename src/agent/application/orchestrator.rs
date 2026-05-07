use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use nu_protocol::{LabeledError, Span, Value};

use crate::agent::protocol::{
    cancellation::is_llm_call_cancelled,
    contracts::{ConversationRuntime, InteractiveUi, ProgressUi, UiMessageSnapshot},
    event::UiEvent,
};

enum WorkerCommand {
    ExecuteTurn { prompt: String, span: Span },
    Shutdown,
}

struct WorkerProgressUi {
    events: mpsc::Sender<UiEvent>,
    cancel_requested: Arc<AtomicBool>,
}

impl ProgressUi for WorkerProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        let _ = self.events.send(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested.swap(false, Ordering::SeqCst)
    }

    fn cancellation_value(&self, span: Span) -> Option<Value> {
        Some(Value::nothing(span))
    }
}

pub(crate) fn run_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime + Send,
    U: InteractiveUi,
{
    std::thread::scope(|scope| {
        let (worker_cmd_tx, worker_cmd_rx) = mpsc::channel::<WorkerCommand>();
        let (worker_event_tx, worker_event_rx) = mpsc::channel::<UiEvent>();
        let (worker_result_tx, worker_result_rx) =
            mpsc::channel::<Result<Value, LabeledError>>();
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel_requested);

        let worker = scope.spawn(move || {
            let mut worker_ui = WorkerProgressUi {
                events: worker_event_tx,
                cancel_requested: worker_cancel,
            };

            while let Ok(command) = worker_cmd_rx.recv() {
                match command {
                    WorkerCommand::ExecuteTurn { prompt, span } => {
                        let result = runtime.execute_turn(&mut worker_ui, prompt, None, span);
                        let _ = worker_result_tx.send(result);
                    }
                    WorkerCommand::Shutdown => break,
                }
            }
        });

        let mut worker_active = false;

        loop {
            ui.pump_once();

            if ui.take_cancel_requested() {
                cancel_requested.store(true, Ordering::SeqCst);
            }

            while let Ok(event) = worker_event_rx.try_recv() {
                ui.emit(&event);
            }

            while let Ok(result) = worker_result_rx.try_recv() {
                worker_active = false;
                match result {
                    Ok(_) => {}
                    Err(error) if is_llm_call_cancelled(&error) => {}
                    Err(error) => {
                        let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                        return Err(error);
                    }
                }
            }

            if let Some(error) = ui.fatal_error() {
                let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                return Err(LabeledError::new(format!("Interactive UI failed: {error}")));
            }

            if !worker_active {
                while let Some(prompt) = ui.take_submitted_prompt() {
                    if prompt.trim().is_empty() {
                        continue;
                    }

                    worker_cmd_tx
                        .send(WorkerCommand::ExecuteTurn { prompt, span })
                        .map_err(|_| {
                            LabeledError::new("Interactive worker channel closed unexpectedly")
                        })?;
                    worker_active = true;
                    break;
                }
            }

            if ui.quit_requested() {
                if worker_active {
                    cancel_requested.store(true, Ordering::SeqCst);
                    continue;
                }
                break;
            }
        }

        let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
        let _ = worker;

        Ok(Value::nothing(span))
    })
}

pub(crate) fn run_hydrated_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime + Send,
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
