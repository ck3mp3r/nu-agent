//! Session store construction tests for `build_acp_runtime()`.

use agnt_core::session::SessionStore;

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_runtime_create_acp_store_creates_sqlite_store() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    // SAFETY: nextest gives each test its own process, so no other thread
    // reads the environment concurrently.
    unsafe { std::env::set_var("XDG_CACHE_HOME", temp.path()) };

    // -- Exec
    let store = create_acp_store()
        .await
        .map_err(|e| format!("should create the ACP store: {e:?}"))?;

    // -- Check
    let db = temp.path().join("nu-agent").join("sessions.db");
    assert!(
        db.exists(),
        "the ACP store must be created under the XDG cache dir at {}",
        db.display()
    );
    assert!(
        store.list().await.map_err(|e| format!("{e:?}"))?.is_empty(),
        "a fresh store must hold no sessions"
    );
    Ok(())
}
