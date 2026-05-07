use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::agent::ui::tui::{
    interaction::{
        cancel::CancelController,
        reducer::{ReducerInput, UserAction, reduce_with_cancel_controller},
    },
    state::AppState,
};

#[test]
fn request_cancel_is_idempotent() {
    let controller = CancelController::new();

    assert!(controller.request_cancel());
    assert!(!controller.request_cancel());
    assert!(controller.is_cancel_requested());
}

#[test]
fn cancel_request_stays_visible_until_finalize_path() {
    let controller = CancelController::new();
    controller.request_cancel();

    assert!(controller.is_cancel_requested());
}

#[test]
fn repeated_request_after_initial_cancel_is_idempotent() {
    let controller = CancelController::new();
    assert!(controller.request_cancel());
    assert!(!controller.request_cancel());
    assert!(controller.is_cancel_requested());
}

#[test]
fn cross_thread_request_is_visible_to_consumer() {
    let controller = CancelController::new();
    let controller_for_thread = controller.clone();
    let (tx, rx) = mpsc::channel::<()>();

    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        controller_for_thread.request_cancel();
        tx.send(()).expect("send completion signal");
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    while !controller.is_cancel_requested() && Instant::now() < deadline {
        thread::yield_now();
    }

    assert!(controller.is_cancel_requested());

    rx.recv_timeout(Duration::from_secs(1))
        .expect("producer completion signal");
    producer.join().expect("producer joins cleanly");
}

#[test]
fn reducer_second_escape_triggers_cancel_request() {
    let cancel_controller = CancelController::new();
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('w')),
        Some(&cancel_controller),
    );
    for ch in "ork".chars() {
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::InsertChar(ch)),
            Some(&cancel_controller),
        );
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::Submit),
        Some(&cancel_controller),
    );

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::Esc),
        Some(&cancel_controller),
    );
    assert!(!cancel_controller.is_cancel_requested());

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EscConfirm),
        Some(&cancel_controller),
    );

    assert!(cancel_controller.is_cancel_requested());
}
