use crate::commands::agent::ui::spinner::SpinnerState;

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
    assert!(s.is_suspended());
    s.resume();
    assert!(!s.is_suspended());
    s.stop();
    assert!(!s.is_active());
    assert!(!s.is_suspended());
}
