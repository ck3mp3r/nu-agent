use std::{cell::RefCell, rc::Rc};

use crate::platform::terminal::{
    TerminalAction, TerminalBackend, TerminalLifecycle, TerminalLifecycleError,
};

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

#[derive(Clone)]
struct MockTerminalBackend {
    actions: Rc<RefCell<Vec<TerminalAction>>>,
    state: Rc<RefCell<MockTerminalState>>,
    fail_on: Option<TerminalAction>,
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

    fn run(&self, action: TerminalAction) -> Result<(), TerminalLifecycleError> {
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
    fn enable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = true;
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::DisableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = false;
        Ok(())
    }

    fn enter_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnterAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = true;
        Ok(())
    }

    fn leave_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::LeaveAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = false;
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::HideCursor)?;
        self.state.borrow_mut().cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::ShowCursor)?;
        self.state.borrow_mut().cursor_visible = true;
        Ok(())
    }

    fn enable_bracketed_paste(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnableBracketedPaste)?;
        self.state.borrow_mut().bracketed_paste_enabled = true;
        Ok(())
    }

    fn disable_bracketed_paste(&mut self) -> Result<(), TerminalLifecycleError> {
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
fn enter_then_restore_uses_required_operation_order() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    lifecycle.enter().expect("enter should succeed");
    lifecycle.restore().expect("restore should succeed");

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
fn restore_is_idempotent_and_safe_to_call_multiple_times() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    lifecycle.enter().expect("enter should succeed");
    lifecycle.restore().expect("first restore should succeed");
    lifecycle.restore().expect("second restore should be no-op");

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
fn enter_failure_recovers_partial_state_and_followup_restore_is_noop() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(
        actions.clone(),
        state.clone(),
        Some(TerminalAction::HideCursor),
    );
    let mut lifecycle = TerminalLifecycle::new(backend);

    let error = lifecycle
        .enter()
        .expect_err("enter should fail on hide_cursor");
    assert_eq!(error.action, TerminalAction::HideCursor);

    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableRawMode,
        ]
    );

    lifecycle
        .restore()
        .expect("restore after recovery should be a no-op");
    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
}

#[test]
fn enter_failure_before_full_init_keeps_terminal_in_restored_state() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(
        actions.clone(),
        state.clone(),
        Some(TerminalAction::EnableRawMode),
    );
    let mut lifecycle = TerminalLifecycle::new(backend);

    let error = lifecycle
        .enter()
        .expect_err("enter should fail before terminal is initialized");
    assert_eq!(error.action, TerminalAction::EnableRawMode);

    assert_eq!(*actions.borrow(), vec![TerminalAction::EnableRawMode]);

    lifecycle
        .restore()
        .expect("restore after pre-init failure should be a no-op");
    assert_terminal_restored(&state);
}
