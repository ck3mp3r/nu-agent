use std::{
    cell::RefCell,
    panic,
    rc::Rc,
};

use crate::commands::agent::ui::tui::platform::safety::{
    RestoreRunError,
    TerminalRestorer,
    run_with_restore,
};

#[derive(Clone)]
struct MockRestorer {
    restore_calls: Rc<RefCell<usize>>,
    fail_restore: bool,
}

impl MockRestorer {
    fn new(restore_calls: Rc<RefCell<usize>>, fail_restore: bool) -> Self {
        Self {
            restore_calls,
            fail_restore,
        }
    }
}

impl TerminalRestorer for MockRestorer {
    type Error = &'static str;

    fn restore_terminal(&mut self) -> Result<(), Self::Error> {
        *self.restore_calls.borrow_mut() += 1;

        if self.fail_restore {
            return Err("restore failed");
        }

        Ok(())
    }
}

#[test]
fn success_path_attempts_restore_and_returns_success() {
    let restore_calls = Rc::new(RefCell::new(0usize));
    let mut restorer = MockRestorer::new(restore_calls.clone(), false);

    let result = run_with_restore(&mut restorer, || Ok::<_, &'static str>("ok"));

    assert_eq!(result.expect("success should propagate"), "ok");
    assert_eq!(*restore_calls.borrow(), 1);
}

#[test]
fn error_path_attempts_restore_and_returns_original_error_when_restore_succeeds() {
    let restore_calls = Rc::new(RefCell::new(0usize));
    let mut restorer = MockRestorer::new(restore_calls.clone(), false);

    let result = run_with_restore::<(), _, _, _>(&mut restorer, || Err("run failed"));

    assert_eq!(
        result.expect_err("error should propagate"),
        RestoreRunError::Run("run failed")
    );
    assert_eq!(*restore_calls.borrow(), 1);
}

#[test]
fn error_path_wraps_error_when_restore_fails() {
    let restore_calls = Rc::new(RefCell::new(0usize));
    let mut restorer = MockRestorer::new(restore_calls.clone(), true);

    let result = run_with_restore::<(), _, _, _>(&mut restorer, || Err("run failed"));

    assert_eq!(
        result.expect_err("combined failure should be returned"),
        RestoreRunError::RunWithRestore {
            run_error: "run failed",
            restore_error: "restore failed",
        }
    );
    assert_eq!(*restore_calls.borrow(), 1);
}

#[test]
fn panic_path_attempts_restore_then_resumes_panic() {
    let restore_calls = Rc::new(RefCell::new(0usize));
    let mut restorer = MockRestorer::new(restore_calls.clone(), false);

    let panic_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let _ = run_with_restore::<(), &'static str, _, _>(&mut restorer, || {
            panic!("boom");
        });
    }));

    assert!(panic_result.is_err(), "panic should be resumed");
    assert_eq!(*restore_calls.borrow(), 1);
}

#[test]
fn panic_path_attempts_restore_even_when_restore_fails_then_resumes_panic() {
    let restore_calls = Rc::new(RefCell::new(0usize));
    let mut restorer = MockRestorer::new(restore_calls.clone(), true);

    let panic_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let _ = run_with_restore::<(), &'static str, _, _>(&mut restorer, || {
            panic!("boom");
        });
    }));

    assert!(panic_result.is_err(), "panic should be resumed");
    assert_eq!(*restore_calls.borrow(), 1);
}
