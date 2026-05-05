use nu_protocol::{LabeledError, Span, Value};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use crate::commands::agent::{
    contracts::{ConversationRuntime, InteractiveUi, ProgressUi, UiMessageSnapshot},
    orchestrator::{run_hydrated_interactive_loop, run_interactive_loop, run_single_turn},
    ui::event::UiEvent,
};

#[derive(Default)]
struct FakeProgressUi {
    events: Vec<UiEvent>,
}

impl ProgressUi for FakeProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

struct FakeInteractiveUi {
    submitted: std::collections::VecDeque<String>,
    quit: bool,
    call_order: Vec<&'static str>,
    hydrated_messages: Vec<UiMessageSnapshot>,
}

impl FakeInteractiveUi {
    fn with_prompts(prompts: &[&str]) -> Self {
        Self {
            submitted: prompts.iter().map(|s| s.to_string()).collect(),
            quit: false,
            call_order: Vec::new(),
            hydrated_messages: Vec::new(),
        }
    }
}

impl ProgressUi for FakeInteractiveUi {
    fn emit(&mut self, _event: &UiEvent) {}

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl InteractiveUi for FakeInteractiveUi {
    fn pump_once(&mut self) {
        self.call_order.push("pump_once");
        if self.submitted.is_empty() {
            self.quit = true;
        }
    }

    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }

    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
        self.call_order.push("hydrate");
        self.hydrated_messages.extend(messages);
    }
}

#[derive(Default)]
struct FakeRuntime {
    prompts: Vec<String>,
}

impl ConversationRuntime for FakeRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::nothing(Span::test_data()))
    }
}

#[test]
fn run_single_turn_uses_progress_ui_trait_boundary() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeProgressUi::default();

    let value = run_single_turn(
        &mut runtime,
        &mut ui,
        "hello".to_string(),
        Some("ctx".to_string()),
        Span::test_data(),
    )
    .expect("single turn");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[test]
fn run_interactive_loop_uses_interactive_ui_trait_boundary() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["a", "b"]);

    let value =
        run_interactive_loop(&mut runtime, &mut ui, Span::test_data()).expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["a".to_string(), "b".to_string()]);
}

#[derive(Default)]
struct FakeValueRuntime {
    prompts: Vec<String>,
}

impl ConversationRuntime for FakeValueRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::record(nu_protocol::Record::new(), span))
    }
}

#[test]
fn interactive_loop_does_not_return_per_turn_values_to_stdout() {
    let mut runtime = FakeValueRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["hello"]);

    let value =
        run_interactive_loop(&mut runtime, &mut ui, Span::test_data()).expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[derive(Default)]
struct CancelFirstRuntime {
    prompts: Vec<String>,
}

impl ConversationRuntime for CancelFirstRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("LLM call cancelled"));
        }

        Ok(Value::nothing(Span::test_data()))
    }
}

#[test]
fn interactive_loop_treats_llm_cancellation_as_non_fatal_and_continues() {
    let mut runtime = CancelFirstRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["first", "second"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data())
        .expect("interactive loop should continue after cancellation");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
}

#[test]
fn run_hydrated_interactive_loop_hydrates_before_first_pump() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let messages = vec![
        UiMessageSnapshot::new("user", "from history"),
        UiMessageSnapshot::new("assistant", "from assistant"),
    ];

    let value = run_hydrated_interactive_loop(&mut runtime, &mut ui, messages, Span::test_data())
        .expect("interactive loop with hydration");

    assert!(value.is_nothing());
    assert_eq!(
        ui.call_order,
        vec!["hydrate", "pump_once"],
        "expected hydrate before first pump"
    );
}

#[test]
fn run_hydrated_interactive_loop_hydrates_exactly_once() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let messages = vec![UiMessageSnapshot::new("user", "history")];
    run_hydrated_interactive_loop(&mut runtime, &mut ui, messages.clone(), Span::test_data())
        .expect("interactive loop with hydration");

    assert_eq!(ui.hydrated_messages, messages);
}

#[derive(Default)]
struct LongRunningRuntime {
    prompts: Arc<Mutex<Vec<String>>>,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
}

impl LongRunningRuntime {
    fn new(block_first_turn: Arc<AtomicBool>) -> Self {
        Self {
            prompts: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            block_first_turn,
        }
    }
}

impl ConversationRuntime for LongRunningRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);
        self.prompts
            .lock()
            .expect("prompts lock")
            .push(prompt.clone());

        if prompt == "first" {
            while !self.block_first_turn.load(Ordering::SeqCst) {
                if ui.take_cancel_requested() {
                    self.active.store(false, Ordering::SeqCst);
                    return Err(LabeledError::new("LLM call cancelled"));
                }
                ui.emit(&UiEvent::Tick);
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(Span::test_data()))
    }
}

struct ResponsiveInteractiveUi {
    submitted: std::collections::VecDeque<String>,
    injected_during_active: Vec<String>,
    injected: bool,
    quit: bool,
    fatal: Option<String>,
    pump_count: usize,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    completed_count: usize,
    expected_completions: usize,
    active_pump_count: Arc<AtomicUsize>,
}

impl ResponsiveInteractiveUi {
    fn new(
        initial_prompts: &[&str],
        injected_during_active: &[&str],
        active: Arc<AtomicBool>,
        block_first_turn: Arc<AtomicBool>,
        expected_completions: usize,
        active_pump_count: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            injected_during_active: injected_during_active.iter().map(|s| s.to_string()).collect(),
            injected: false,
            quit: false,
            fatal: None,
            pump_count: 0,
            active,
            block_first_turn,
            completed_count: 0,
            expected_completions,
            active_pump_count,
        }
    }
}

impl ProgressUi for ResponsiveInteractiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Completed { .. } = event {
            self.completed_count += 1;
            if self.completed_count >= self.expected_completions {
                self.quit = true;
            }
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl InteractiveUi for ResponsiveInteractiveUi {
    fn pump_once(&mut self) {
        self.pump_count += 1;
        if self.active.load(Ordering::SeqCst) {
            self.active_pump_count.fetch_add(1, Ordering::SeqCst);
            if !self.injected {
                for prompt in self.injected_during_active.clone() {
                    self.submitted.push_back(prompt);
                }
                self.injected = true;
                // Deterministic unblock: once we've injected prompts during active work,
                // allow the first turn to complete so queued prompts can proceed.
                self.block_first_turn.store(true, Ordering::SeqCst);
            }
        }
    }

    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }

    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
    }
}

#[test]
fn interactive_loop_processes_input_while_first_turn_is_running() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        2,
        Arc::clone(&active_pump_count),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data())
        .expect("interactive loop should stay responsive");

    assert!(value.is_nothing());
    assert!(
        active_pump_count.load(Ordering::SeqCst) > 0,
        "expected UI pump to continue while runtime turn was active"
    );
    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second"]
    );
}

#[test]
fn interactive_loop_preserves_fifo_for_prompts_queued_while_active() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second", "third"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        3,
        Arc::clone(&active_pump_count),
    );

    let _ = run_interactive_loop(&mut runtime, &mut ui, Span::test_data())
        .expect("interactive loop should complete queued prompts");

    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second", "third"]
    );
}

struct AbortDuringActiveUi {
    submitted: std::collections::VecDeque<String>,
    quit: bool,
    fatal: Option<String>,
    esc_stage: u8,
    active: Arc<AtomicBool>,
    cancel_requested: Arc<AtomicBool>,
    completed_count: usize,
}

impl AbortDuringActiveUi {
    fn new(active: Arc<AtomicBool>) -> Self {
        Self {
            submitted: ["first".to_string(), "queued".to_string()].into_iter().collect(),
            quit: false,
            fatal: None,
            esc_stage: 0,
            active,
            cancel_requested: Arc::new(AtomicBool::new(false)),
            completed_count: 0,
        }
    }
}

impl ProgressUi for AbortDuringActiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Completed { .. } = event {
            self.completed_count += 1;
            if self.completed_count > 0 {
                self.quit = true;
            }
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested.swap(false, Ordering::SeqCst)
    }
}

impl InteractiveUi for AbortDuringActiveUi {
    fn pump_once(&mut self) {
        if self.active.load(Ordering::SeqCst) && self.esc_stage < 2 {
            self.esc_stage += 1;
            if self.esc_stage == 2 {
                self.cancel_requested.store(true, Ordering::SeqCst);
                self.submitted.clear();
            }
        }

        // Deterministic termination: once cancellation has been requested and
        // active work is no longer running, allow loop exit even if no
        // Completed event is emitted for cancellation.
        if self.esc_stage == 2 && !self.active.load(Ordering::SeqCst) {
            self.quit = true;
        }
    }

    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }

    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
    }
}

#[test]
fn interactive_loop_global_abort_cancels_active_and_does_not_run_queued_prompt() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let mut ui = AbortDuringActiveUi::new(Arc::clone(&runtime.active));

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data())
        .expect("interactive loop should treat cancellation as non-fatal");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
}
