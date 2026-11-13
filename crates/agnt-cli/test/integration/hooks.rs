//! Integration tests for the user hook wiring in the CLI modes.
//!
//! The zero-hook and configured-hook cases drive `mode::spawn_configured_hooks`
//! against a real config.toml in a throwaway XDG tree. The end-to-end case runs
//! `dispatch::run_agent` in stderr mode with a real `nu` hook script, so the
//! whole path — config, resolution, runner, subprocess, envelope — is exercised.

use super::*;

use agnt_runtime::bus::LifecycleEvent;
use agnt_runtime::hooks::HookProcessError;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Records every hook invocation and always succeeds.
#[derive(Clone, Default)]
struct RecordingProcess {
    calls: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

impl RecordingProcess {
    fn calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl agnt_runtime::hooks::HookProcess for RecordingProcess {
    async fn run(
        &self,
        _script: &std::path::Path,
        event: &str,
        envelope: &str,
        _cwd: &std::path::Path,
        _timeout: std::time::Duration,
    ) -> core::result::Result<(), HookProcessError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((event.to_string(), envelope.to_string()));
        }
        Ok(())
    }
}

/// Write a hook script that appends every envelope it receives to `log`.
fn write_hook_script(env: &support::TestEnv, log: &std::path::Path) -> Result<String> {
    let dir = env
        .config_path()
        .parent()
        .ok_or("config path has no parent")?
        .to_path_buf();
    std::fs::create_dir_all(&dir)?;
    let script = dir.join("audit.nu");
    let log = log.to_string_lossy().to_string();
    std::fs::write(
        &script,
        format!(
            "def turn_started [event] {{ $event | to json | save --append '{log}' }}\n\
             def turn_completed [event] {{ $event | to json | save --append '{log}' }}\n"
        ),
    )?;
    Ok("audit.nu".to_string())
}

/// The lines of the hook log, or an empty vec when the file does not exist.
fn hook_log_lines(log: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .map(|body| body.lines().map(|line| line.to_string()).collect())
        .unwrap_or_default()
}

/// The hook log, once `ready` accepts it or the deadline passes.
///
/// The mode's exit drain is best-effort: a hook subprocess gets a short budget
/// and may still be starting when the turn returns, so reading the log the
/// instant the turn ends races the writer. Waiting for the effect separates a
/// slow `nu` start from a hook that never ran — a hook that fails or never fires
/// still fails at the deadline.
///
async fn wait_for_hook_lines(
    log: &std::path::Path,
    ready: impl Fn(&[String]) -> bool,
) -> Vec<String> {
    let deadline = std::time::Instant::now() + HOOK_LOG_DEADLINE;
    loop {
        let lines = hook_log_lines(log);
        if ready(&lines) || std::time::Instant::now() >= deadline {
            return lines;
        }
        tokio::time::sleep(HOOK_LOG_POLL).await;
    }
}

/// How long the hook log has to show the effect the turn published.
const HOOK_LOG_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

/// How often the wait looks again.
const HOOK_LOG_POLL: std::time::Duration = std::time::Duration::from_millis(25);

// endregion: --- Test Support

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_hooks_zero_hooks_spawns_no_runner() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let config = agnt_runtime::config::toml_config::load_from(&env.config_path())?;
    let bus = agnt_runtime::bus::Bus::default();
    let process = RecordingProcess::default();

    // -- Exec
    let handles = crate::mode::spawn_configured_hooks(
        Some(&config),
        agnt_runtime::hooks::HookSources {
            stream: bus.stream(),
            lifecycle: bus.lifecycle(),
        },
        env.config_path()
            .parent()
            .ok_or("config path has no parent")?,
        env.config_path()
            .parent()
            .ok_or("config path has no parent")?,
        &agnt_runtime::hooks::HookAgentContext::default(),
        process.clone(),
    );

    // -- Check
    assert!(
        handles.is_empty(),
        "a config without [[hooks]] must spawn no hook runner"
    );
    assert!(process.calls().is_empty(), "no hook process may be invoked");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_hooks_configured_hooks_start_a_runner() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_config(&format!(
        "[models.default]\nmodel = \"ollama/llama3\"\n\n[providers.ollama]\nbase_url = \"{}\"\n\n[[hooks]]\nname = \"audit\"\nevents = [\"turn_started\"]\nscript = \"audit.nu\"\n",
        env.llm().uri()
    ))?;
    let config = agnt_runtime::config::toml_config::load_from(&env.config_path())?;
    let bus = agnt_runtime::bus::Bus::default();
    let process = RecordingProcess::default();
    let config_dir = env
        .config_path()
        .parent()
        .ok_or("config path has no parent")?
        .to_path_buf();

    // -- Exec
    let handles = crate::mode::spawn_configured_hooks(
        Some(&config),
        agnt_runtime::hooks::HookSources {
            stream: bus.stream(),
            lifecycle: bus.lifecycle(),
        },
        &config_dir,
        &config_dir,
        &agnt_runtime::hooks::HookAgentContext::default(),
        process.clone(),
    );
    let _ = bus.lifecycle().publish(LifecycleEvent::TurnStarted {
        prompt: "hello".to_string(),
        task_id: None,
    });
    drop(bus);
    for handle in handles {
        handle.await?;
    }

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "the configured hook must run once");
    assert_eq!(calls[0].0, "turn_started");
    assert!(
        calls[0].1.contains("hello"),
        "the envelope must carry the prompt; got: {}",
        calls[0].1
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_hooks_stderr_mode_runs_the_hook_script() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    let log = env.data_home_path("hook.log");
    let script = write_hook_script(&env, &log)?;
    env.write_config(&format!(
        "[models.default]\nmodel = \"ollama/llama3\"\n\n[providers.ollama]\nbase_url = \"{}\"\n\n[[hooks]]\nname = \"audit\"\nevents = [\"turn_started\", \"turn_completed\"]\nscript = \"{script}\"\n",
        env.llm().uri()
    ))?;
    agnt_testkit::mount_text_response(env.llm(), "hello from mock", None).await;
    let cli = support::parse_cli(&["hello"])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the stderr turn must complete: {e}"))?;

    // -- Check
    // The turn-dispatch layer publishes `LifecycleEvent::TurnStarted` before the turn
    // runs (`run_single_turn` on this path, the worker router on the
    // interactive path) and the turn executor publishes `LifecycleEvent::TurnCompleted`
    // after it. The hook must observe both.
    let lines = wait_for_hook_lines(&log, |lines| {
        lines.iter().any(|line| line.contains("\"turn_started\""))
            && lines.iter().any(|line| line.contains("\"turn_completed\""))
    })
    .await;
    assert!(
        lines.iter().any(|line| line.contains("\"turn_started\"")),
        "the hook must observe turn_started; got: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("\"turn_completed\"")),
        "the hook must observe turn_completed; got: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("\"version\": 1")),
        "the envelope must carry the v1 header; got: {lines:?}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_hooks_stderr_mode_without_hooks_writes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    let log = env.data_home_path("hook.log");
    env.write_default_config()?;
    agnt_testkit::mount_text_response(env.llm(), "hello from mock", None).await;
    let cli = support::parse_cli(&["hello"])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the stderr turn must complete: {e}"))?;

    // -- Check
    assert!(
        hook_log_lines(&log).is_empty(),
        "a config without [[hooks]] must not run any hook"
    );
    Ok(())
}

// endregion: --- Tests
