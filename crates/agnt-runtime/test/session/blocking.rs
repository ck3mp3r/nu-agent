//! The blocking-pool bridge: what it carries, and how it fails.

use crate::session::blocking::{PoolError, on_pool};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

/// The work's own value and error come back unchanged.
#[tokio::test]
async fn test_blocking_on_pool_answers_the_work_result() -> Result<()> {
    // -- Setup & Fixtures
    let value = 41;

    // -- Exec
    let answered = on_pool(move || Ok::<i64, String>(value + 1)).await;
    let refused = on_pool(|| Err::<i64, String>("the store refused".to_owned())).await;

    // -- Check
    assert_eq!(answered.map_err(|error| error.to_string())?, Ok(42));
    assert_eq!(
        refused.map_err(|error| error.to_string())?,
        Err("the store refused".to_owned()),
        "the work's own error keeps its family"
    );
    Ok(())
}

/// A closure that panics surfaces as a pool error carrying what it panicked
/// with: a facade never swallows it and never unwraps it.
#[tokio::test]
async fn test_blocking_on_pool_carries_the_panic_payload() -> Result<()> {
    // -- Setup & Fixtures
    let session = "session-panic";

    // -- Exec
    let refused = on_pool(move || -> core::result::Result<(), String> {
        panic!("the store of {session} is gone");
    })
    .await;

    // -- Check
    let error = match refused {
        Ok(_) => return Err("a panicking closure SHALL NOT answer".into()),
        Err(error) => error,
    };
    let PoolError::Panicked { payload } = &error else {
        return Err(format!("expected a panic, got {error}").into());
    };
    assert_eq!(payload, "the store of session-panic is gone");
    assert!(
        error
            .to_string()
            .contains("the store of session-panic is gone"),
        "the message carries the payload: {error}"
    );
    Ok(())
}

/// Aborting the work surfaces as a cancellation, not as a panic.
#[tokio::test]
async fn test_blocking_pool_error_maps_a_cancelled_task() -> Result<()> {
    // -- Setup & Fixtures
    let worker = tokio::spawn(async {
        std::future::pending::<()>().await;
    });

    // -- Exec
    worker.abort();
    let answered = worker.await;

    // -- Check
    let error = answered.err().ok_or("an aborted task SHALL fail")?;
    assert!(
        matches!(PoolError::from(error), PoolError::Cancelled),
        "an abort is a cancellation"
    );
    Ok(())
}

/// The work runs while the runtime keeps driving its own tasks: a call through
/// the bridge does not park the runtime's threads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_blocking_on_pool_leaves_the_runtime_driving_tasks() -> Result<()> {
    // -- Setup & Fixtures
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ticks = std::sync::Arc::clone(&counter);
    let driven = tokio::spawn(async move {
        for _ in 0..8 {
            tokio::task::yield_now().await;
            ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });

    // -- Exec: a blocking-pool op and an independent task, concurrently
    let (answered, driven) = tokio::join!(on_pool(|| Ok::<u8, String>(7u8)), driven);
    driven?;

    // -- Check
    assert_eq!(answered.map_err(|error| error.to_string())?, Ok(7));
    assert_eq!(
        counter.load(std::sync::atomic::Ordering::SeqCst),
        8,
        "the runtime SHALL keep driving tasks while the pool works"
    );
    Ok(())
}

// endregion: --- Tests
