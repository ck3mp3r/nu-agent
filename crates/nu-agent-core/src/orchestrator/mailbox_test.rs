use super::test_shared::*;
use crate::mailbox::IncomingMessage;
use std::sync::{Arc, Mutex};

#[test]
fn mailbox_message_injected_as_turn() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "orchestrator".to_string(),
        message: "implement the login endpoint".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[from: orchestrator] implement the login endpoint".to_string()]
    );
    assert_eq!(
        ui.displayed_incoming_messages,
        vec!["[from: orchestrator] implement the login endpoint".to_string()]
    );
}

#[test]
fn mailbox_clear_resets_session() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "orchestrator".to_string(),
        message: "/clear".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.clear_session_calls, 1);
    assert_eq!(ui.clear_transcript_calls, 1);
    assert_eq!(
        runtime.prompts.len(),
        0,
        "/clear should not be injected as a turn"
    );
}

#[test]
fn mailbox_none_no_change() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&["hello"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    assert_eq!(runtime.clear_session_calls, 0);
}

#[test]
fn user_input_takes_precedence() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&["user prompt"]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "orchestrator".to_string(),
        message: "mailbox prompt".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    // User prompt should be processed first
    assert_eq!(runtime.prompts[0], "user prompt".to_string());
}

#[test]
fn mailbox_queued_when_worker_busy() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let prompts_clone = Arc::clone(&prompts);

    struct BusyRuntime {
        prompts: Arc<Mutex<Vec<String>>>,
        first_call: std::sync::atomic::AtomicBool,
    }

    impl CoreRuntime for BusyRuntime {
        fn execute_turn<U: ProgressUi>(
            &mut self,
            ui: &mut U,
            prompt: String,
            _context: Option<String>,
            _span: Span,
        ) -> Result<Value, LabeledError> {
            self.prompts.lock().unwrap().push(prompt.clone());

            // Simulate long-running first turn
            if self.first_call.load(Ordering::SeqCst) {
                for _ in 0..10 {
                    ui.emit(&UiEvent::Tick);
                    std::thread::sleep(Duration::from_millis(2));
                }
                self.first_call.store(false, Ordering::SeqCst);
            }

            Ok(Value::nothing(Span::test_data()))
        }
    }

    impl HasMcpManagement for BusyRuntime {
        fn set_mcp_server_enabled(
            &mut self,
            _name: &str,
            _enabled: bool,
        ) -> Result<McpUsabilityState, String> {
            Ok(McpUsabilityState::Disabled)
        }

        fn llm_visible_mcp_tool_count(&self) -> usize {
            0
        }

        fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
            0
        }

        fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
            Vec::new()
        }
    }

    impl HasModelSwitching for BusyRuntime {
        fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
            Err("model switching not supported".to_string())
        }

        fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
            Err("agent switch not supported in this runtime".to_string())
        }

        fn active_model_identity(&self) -> String {
            "unknown/unknown".to_string()
        }

        fn max_context_tokens(&self) -> Option<u64> {
            None
        }
    }

    impl HasSessionManagement for BusyRuntime {}
    impl HasCompaction for BusyRuntime {}

    let mut runtime = BusyRuntime {
        prompts: prompts_clone,
        first_call: std::sync::atomic::AtomicBool::new(true),
    };
    let mut ui = MailboxTestUi::with_prompts(&["first"]);

    let (tx, rx) = std::sync::mpsc::channel();

    // Send mailbox message that will arrive while worker is busy
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        tx.send(IncomingMessage {
            from: "orchestrator".to_string(),
            message: "queued message".to_string(),
            kind: "message".to_string(),
        })
        .ok();
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    let final_prompts = prompts.lock().unwrap();
    assert_eq!(final_prompts.len(), 2);
    assert_eq!(final_prompts[0], "first");
    assert_eq!(final_prompts[1], "[from: orchestrator] queued message");
    assert!(
        ui.displayed_incoming_messages
            .contains(&"[from: orchestrator] queued message".to_string()),
        "deferred mailbox message should be displayed when sent: {:?}",
        ui.displayed_incoming_messages
    );
}
