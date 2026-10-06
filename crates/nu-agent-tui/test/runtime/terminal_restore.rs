use super::*;

#[derive(Clone)]
struct MockTerminalBackend {
    actions: Rc<RefCell<Vec<TerminalAction>>>,
    state: Rc<RefCell<MockTerminalState>>,
    fail_on: Option<TerminalAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MockTerminalState {
    raw_mode_enabled: bool,
    alt_screen_enabled: bool,
    cursor_visible: bool,
    bracketed_paste_enabled: bool,
}

impl Default for MockTerminalState {
    fn default() -> Self {
        Self {
            raw_mode_enabled: false,
            alt_screen_enabled: false,
            cursor_visible: true,
            bracketed_paste_enabled: false,
        }
    }
}

impl MockTerminalBackend {
    fn new(
        actions: Rc<RefCell<Vec<TerminalAction>>>,
        state: Rc<RefCell<MockTerminalState>>,
        fail_on: Option<TerminalAction>,
    ) -> Self {
        Self {
            actions,
            state,
            fail_on,
        }
    }

    fn run(&self, action: TerminalAction) -> core::result::Result<(), TerminalLifecycleError> {
        self.actions.borrow_mut().push(action);

        if self.fail_on == Some(action) {
            return Err(TerminalLifecycleError::new(
                action,
                format!("injected failure for {action:?}"),
            ));
        }

        Ok(())
    }
}

impl TerminalBackend for MockTerminalBackend {
    fn enable_raw_mode(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = true;
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::DisableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = false;
        Ok(())
    }

    fn enter_alt_screen(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnterAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = true;
        Ok(())
    }

    fn leave_alt_screen(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::LeaveAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = false;
        Ok(())
    }

    fn hide_cursor(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::HideCursor)?;
        self.state.borrow_mut().cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::ShowCursor)?;
        self.state.borrow_mut().cursor_visible = true;
        Ok(())
    }

    fn enable_bracketed_paste(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnableBracketedPaste)?;
        self.state.borrow_mut().bracketed_paste_enabled = true;
        Ok(())
    }

    fn disable_bracketed_paste(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::DisableBracketedPaste)?;
        self.state.borrow_mut().bracketed_paste_enabled = false;
        Ok(())
    }
}

fn assert_terminal_restored(state: &Rc<RefCell<MockTerminalState>>) {
    assert_eq!(
        *state.borrow(),
        MockTerminalState {
            raw_mode_enabled: false,
            alt_screen_enabled: false,
            cursor_visible: true,
            bracketed_paste_enabled: false,
        }
    );
}

#[test]
fn run_with_terminal_restore_sync_executes_enter_run_and_restore_in_order() -> Result<()> {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let value = run_with_terminal_restore_sync(&mut lifecycle, || Ok::<_, &'static str>(42))
        .map_err(|e| format!("run should succeed: {e:?}"))?;
    assert_eq!(value, 42);

    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
    Ok(())
}

#[test]
fn runtime_enter_failure_maps_to_enter_error() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(
        actions.clone(),
        state.clone(),
        Some(TerminalAction::EnterAltScreen),
    );
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || Ok(()))
        .expect_err("expected enter failure");

    match err {
        RuntimeRunError::Enter(error) => {
            assert_eq!(error.action, TerminalAction::EnterAltScreen);
        }
        RuntimeRunError::Run(run_error) => panic!("unexpected run error: {run_error:?}"),
    }

    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
}

#[test]
fn runtime_run_error_maps_to_run() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions, state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), _, _>(&mut lifecycle, || Err("boom"))
        .expect_err("expected run failure");

    match err {
        RuntimeRunError::Run(RestoreRunError::Run(run_error)) => assert_eq!(run_error, "boom"),
        RuntimeRunError::Enter(error) => {
            panic!("unexpected enter error: {error}")
        }
        RuntimeRunError::Run(other) => panic!("unexpected run mapping: {other:?}"),
    }
    assert_terminal_restored(&state);
}

#[test]
fn runtime_run_and_restore_error_maps_to_run_with_restore() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions, state, Some(TerminalAction::ShowCursor));
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), _, _>(&mut lifecycle, || Err("boom"))
        .expect_err("expected combined run/restore failure");

    match err {
        RuntimeRunError::Run(RestoreRunError::RunWithRestore {
            run_error,
            restore_error,
        }) => {
            assert_eq!(run_error, "boom");
            assert_eq!(restore_error.action, TerminalAction::ShowCursor);
        }
        other => panic!("unexpected runtime error mapping: {other:?}"),
    }
}

#[test]
fn runtime_restore_error_after_success_maps_to_restore() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions, state, Some(TerminalAction::ShowCursor));
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || Ok(()))
        .expect_err("expected restore failure");

    match err {
        RuntimeRunError::Run(RestoreRunError::Restore(restore_error)) => {
            assert_eq!(restore_error.action, TerminalAction::ShowCursor);
        }
        other => panic!("unexpected runtime error mapping: {other:?}"),
    }
}

#[test]
fn runtime_panic_restores_and_rethrows() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || {
            panic!("boom");
        });
    }));

    assert!(panic_result.is_err(), "panic should be resumed");
    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
}

#[test]
fn panic_during_busy_render_loop_still_restores_terminal_state() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || {
            panic!("simulated panic during busy render loop")
        });
    }));

    assert!(panic_result.is_err(), "panic should be resumed");
    assert_terminal_restored(&state);
    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
}

#[test]
fn cancellation_during_shutdown_restores_terminal_and_preserves_cancel_error() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), _, _>(&mut lifecycle, || {
        Err("cancelled during shutdown")
    })
    .expect_err("expected cancellation error");

    match err {
        RuntimeRunError::Run(RestoreRunError::Run(run_error)) => {
            assert_eq!(run_error, "cancelled during shutdown");
        }
        other => panic!("unexpected runtime error mapping: {other:?}"),
    }

    assert_terminal_restored(&state);
    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
}
