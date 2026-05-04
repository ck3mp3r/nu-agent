use nu_protocol::{LabeledError, Span, Value};

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
