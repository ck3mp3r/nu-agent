//! Integration tests for the user hook wiring in the CLI modes.
//!
//! The zero-hook and configured-hook cases drive `mode::spawn_configured_hooks`
//! against a real config.toml in a throwaway XDG tree. The end-to-end case runs
//! `dispatch::run_agent` in stderr mode with a real `nu` hook script, so the
//! whole path — config, resolution, runner, subprocess, envelope — is exercised.

use super::*;

use agnt_core::bus::TurnEvent;
use agnt_core::hooks::HookProcessError;

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

impl agnt_core::hooks::HookProcess for RecordingProcess {
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

// endregion: --- Test Support

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_hooks_zero_hooks_spawns_no_runner() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let config = agnt_core::config::toml_config::load_from(&env.config_path())?;
    let bus = agnt_core::bus::create_bus();
    let process = RecordingProcess::default();

    // -- Exec
    let handles = crate::mode::spawn_configured_hooks(
        Some(&config),
        &bus,
        env.config_path()
            .parent()
            .ok_or("config path has no parent")?,
        &agnt_core::hooks::HookAgentContext::default(),
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
    let config = agnt_core::config::toml_config::load_from(&env.config_path())?;
    let bus = agnt_core::bus::create_bus();
    let process = RecordingProcess::default();
    let config_dir = env
        .config_path()
        .parent()
        .ok_or("config path has no parent")?
        .to_path_buf();

    // -- Exec
    let handles = crate::mode::spawn_configured_hooks(
        Some(&config),
        &bus,
        &config_dir,
        &agnt_core::hooks::HookAgentContext::default(),
        process.clone(),
    );
    let _ = bus
        .turn()
        .send(TurnEvent::Started {
            prompt: "hello".to_string(),
            task_id: None,
        })
        .await;
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
    // The turn-dispatch layer publishes `TurnEvent::Started` before the turn
    // runs (`run_single_turn` on this path, the worker router on the
    // interactive path) and the turn executor publishes `TurnEvent::Completed`
    // after it. The hook must observe both.
    let lines = hook_log_lines(&log);
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
