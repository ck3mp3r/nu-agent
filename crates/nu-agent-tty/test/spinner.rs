use crate::spinner::SpinnerState;

#[test]
fn spinner_only_runs_when_enabled() {
    let mut disabled = SpinnerState::new(false);
    disabled.start();
    assert!(!disabled.is_active());

    let mut enabled = SpinnerState::new(true);
    enabled.start();
    assert!(enabled.is_active());
}

#[test]
fn spinner_tick_advances_frames_when_active_and_not_suspended() {
    let mut spinner = SpinnerState::new_with_charset(true, false);
    spinner.start();
    let first = spinner.current_frame().to_string();
    spinner.tick();
    let second = spinner.current_frame().to_string();
    assert_ne!(first, second);

    spinner.suspend();
    let suspended = spinner.current_frame().to_string();
    spinner.tick();
    assert_eq!(suspended, spinner.current_frame());
}

#[test]
fn spinner_supports_ascii_fallback_frames() {
    let mut spinner = SpinnerState::new_with_charset(true, false);
    spinner.start();
    assert_eq!(spinner.current_frame(), "-");
    spinner.tick();
    assert_eq!(spinner.current_frame(), "\\");
}

#[test]
fn spinner_suspend_resume_lifecycle_is_safe() {
    let mut s = SpinnerState::new(true);
    s.start();
    s.suspend();
    let suspended_frame = s.current_frame().to_string();
    s.tick();
    assert_eq!(
        suspended_frame,
        s.current_frame(),
        "suspended spinner must not advance"
    );
    s.resume();
    s.tick();
    assert_ne!(
        suspended_frame,
        s.current_frame(),
        "resumed spinner should advance"
    );
    s.stop();
    assert!(!s.is_active());
}
