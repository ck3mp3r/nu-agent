use crate::tools::handler::ToolErrorKind;
use crate::tools::handler::builtin_tool::BuiltinTool;
use crate::tools::handler::nu::NuTool;
use agnt_kernel::bus::{CancelEvent, CancelTx};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value as JsonValue;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A killed group vanishes in sub-millisecond time; one second is the
/// ceiling the acceptance criteria allow.
const KILL_WINDOW: Duration = Duration::from_secs(1);
/// Upper bound on waiting for the spawned command to publish its child pid.
const PID_FILE_WINDOW: Duration = Duration::from_secs(10);
/// The configured timeout (1 s) plus the 1 s grace the criteria allow.
const TIMEOUT_WINDOW: Duration = Duration::from_secs(2);

#[tokio::test]
async fn nu_cancel_kills_process_tree() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let pid_file = temp.path().join("child.pid");
    let args = tree_args(&pid_file);
    let cwd = temp.path().to_path_buf();
    let cancel = CancelTx::new("test", 8);
    let cancel_for_tool = cancel.clone();

    // -- Exec
    let call = tokio::spawn(async move { NuTool::execute(&args, &cwd, &cancel_for_tool).await });
    let child_pid = wait_for_child_pid(&pid_file).await?;
    cancel_when_subscribed(&cancel).await;
    let err = call
        .await?
        .err()
        .ok_or("a cancelled call must return an error")?;

    // -- Check
    assert!(
        err.message.contains("cancelled"),
        "message must carry the cancel, got: {}",
        err.message
    );
    exits_within(child_pid, KILL_WINDOW).await?;
    Ok(())
}

#[tokio::test]
async fn nu_timeout_kills_process_tree() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let pid_file = temp.path().join("child.pid");
    let mut args = tree_args(&pid_file);
    args["timeout_seconds"] = serde_json::json!(1);
    let cwd = temp.path().to_path_buf();
    let cancel = CancelTx::new("test", 8);

    // -- Exec
    let call = tokio::spawn(async move { NuTool::execute(&args, &cwd, &cancel).await });
    let child_pid = wait_for_child_pid(&pid_file).await?;
    let err = call
        .await?
        .err()
        .ok_or("a timed-out call must return an error")?;

    // -- Check
    assert!(
        err.message.contains("timed out"),
        "message must carry the timeout, got: {}",
        err.message
    );
    exits_within(child_pid, KILL_WINDOW).await?;
    Ok(())
}

/// A wrapper that exits before its grandchild must not outrun the wall clock:
/// `nu -c '<daemon> &'` returns at once while the daemon keeps the inherited
/// stdout pipe open. The call has to return the not-run timeout error and kill
/// the tree when `timeout_seconds` expires, not when the pipe finally closes.
#[tokio::test]
async fn nu_early_wrapper_exit_still_honours_the_timeout() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let pid_file = temp.path().join("child.pid");
    let mut args = early_wrapper_exit_args(&pid_file);
    args["timeout_seconds"] = serde_json::json!(1);
    let cwd = temp.path().to_path_buf();
    let cancel = CancelTx::new("test", 8);
    let cancel_for_tool = cancel.clone();

    // -- Exec
    let started = Instant::now();
    let call = tokio::spawn(async move { NuTool::execute(&args, &cwd, &cancel_for_tool).await });
    let child_pid = wait_for_child_pid(&pid_file).await?;
    let err = call
        .await?
        .err()
        .ok_or("a timed-out call must return an error")?;
    let elapsed = started.elapsed();

    // -- Check
    assert_eq!(err.kind, ToolErrorKind::Cancelled, "timeout kind: {err:?}");
    assert!(
        err.message.contains("timed out"),
        "message must carry the timeout, got: {}",
        err.message
    );
    assert!(
        elapsed < TIMEOUT_WINDOW,
        "the call must return inside the wall clock, took {} ms",
        elapsed.as_millis()
    );
    exits_within(child_pid, KILL_WINDOW).await?;
    Ok(())
}

/// The turn's `FilteredToolProxy` races its own cancel arm against the tool
/// call and drops the call future when its arm wins. The tree must die anyway.
#[tokio::test]
async fn nu_cancel_kills_process_tree_after_call_future_dropped() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let pid_file = temp.path().join("child.pid");
    let args = tree_args(&pid_file);
    let cwd = temp.path().to_path_buf();
    let cancel = CancelTx::new("test", 8);
    let mut call = Box::pin(NuTool::execute(&args, &cwd, &cancel));

    // -- Exec
    let child_pid = tokio::select! {
        pid = wait_for_child_pid(&pid_file) => pid?,
        result = &mut call => return Err(format!("the call must not finish before the cancel: {result:?}").into()),
    };
    drop(call);
    cancel_when_subscribed(&cancel).await;

    // -- Check
    exits_within(child_pid, KILL_WINDOW).await?;
    Ok(())
}

// region:    --- Test Support

/// Build the args for a wrapper that exits before its grandchild: `nu` runs
/// `sh`, `sh` backgrounds a `sleep` that inherits the stdout pipe and records
/// its pid, then `sh` and `nu` both exit. The pipe stays open for as long as
/// the `sleep` lives, so only a group kill closes it.
fn early_wrapper_exit_args(pid_file: &Path) -> JsonValue {
    let command = format!("^sh -c 'sleep 30 & echo $! > {}'", pid_file.display());
    serde_json::json!({ "command": command })
}

/// Build the args for a command whose process tree outlives the nu wrapper:
/// `nu` spawns `sh`, `sh` spawns `sleep` in the background and records its pid,
/// then blocks in `wait` so the tree stays alive until something kills it.
fn tree_args(pid_file: &Path) -> JsonValue {
    let command = format!("^sh -c 'sleep 30 & echo $! > {}; wait'", pid_file.display());
    serde_json::json!({ "command": command })
}

/// Poll the pid file the command writes. A broadcast-free sync point: the file
/// exists only once the grandchild is running.
async fn wait_for_child_pid(pid_file: &Path) -> Result<i32> {
    let deadline = Instant::now() + PID_FILE_WINDOW;
    loop {
        let published = tokio::fs::read_to_string(pid_file).await.ok();
        if let Some(pid) = published.and_then(|text| text.trim().parse::<i32>().ok()) {
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            return Err("the command must publish its child pid".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Publish the cancel event once the tool subscribed to the channel: a
/// broadcast send succeeds only when a receiver is live. Bounded so a missed
/// subscription cannot hang the test.
async fn cancel_when_subscribed(cancel: &CancelTx) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if cancel.send(CancelEvent::Requested).await.is_ok() {
            return;
        }
        if Instant::now() >= deadline {
            return;
        }
        tokio::task::yield_now().await;
    }
}

/// Assert the pid dies inside `window`.
async fn exits_within(pid: i32, window: Duration) -> Result<()> {
    let deadline = Instant::now() + window;
    loop {
        if !is_alive(pid) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("pid {pid} must exit within {} ms", window.as_millis()).into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn is_alive(pid: i32) -> bool {
    // SAFETY: signal 0 performs the permission and existence check only.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

// endregion: --- Test Support
