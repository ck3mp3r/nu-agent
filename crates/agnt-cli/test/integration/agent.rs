//! Integration tests for `dispatch::run_agent` and `mode::run_stderr_mode`.
//!
//! Every test calls `run_agent` directly with a real config.toml, a real
//! session store, and a wiremock LLM. A positional prompt forces stderr mode,
//! so the tests never depend on the terminal state.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "agent/turn_budget.rs"]
mod turn_budget;

// endregion: --- Modules

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Write a persona markdown file into the XDG config `agents/` directory the
/// CLI resolves personas from.
fn write_persona(env: &support::TestEnv, name: &str, contents: &str) -> Result<()> {
    let dir = env
        .config_path()
        .parent()
        .ok_or("config path has no parent")?
        .join("agents");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{name}.md")), contents)?;
    Ok(())
}

/// Mount a mock that answers every request with a `read` tool call whose
/// `offset` argument advances with each request.
///
/// The advancing offset keeps every tool-call signature distinct, so the
/// doom-loop detector never fires and the turn runs until its tool-call budget
/// is exhausted.
async fn mount_advancing_read_tool(env: &support::TestEnv, path: &str) -> Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let path = path.to_string();
    agnt_testkit::Mock::given(agnt_testkit::matchers::method("POST"))
        .respond_with(move |_request: &agnt_testkit::Request| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let args = serde_json::json!({ "path": path, "offset": call });
            agnt_testkit::sse_response_template(agnt_testkit::sse_tool_call_body(
                &format!("call_{call}"),
                "read",
                &args.to_string(),
            ))
        })
        .mount(env.llm())
        .await;
    Ok(())
}

/// Write a 200-line file into the XDG data home and return its absolute path.
fn write_read_target(env: &support::TestEnv) -> Result<String> {
    let path = env.data_home_path("cov-read-target.txt");
    let body: String = (0..200).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&path, body)?;
    Ok(path.to_string_lossy().to_string())
}

// endregion: --- Test Support

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_stderr_turn_completes() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    agnt_testkit::mount_text_response(env.llm(), "hello from mock", None).await;
    let cli = support::parse_cli(&["hello"])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the stderr turn must complete: {e}"))?;

    // -- Check
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    assert_eq!(
        bodies.len(),
        1,
        "the turn must issue exactly one completion"
    );
    assert!(
        bodies.first().is_some_and(|body| body.contains("hello")),
        "the completion request must carry the prompt; got: {bodies:?}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_applies_cli_flags() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    agnt_testkit::mount_text_response(env.llm(), "flagged", None).await;
    let cli = support::parse_cli(&[
        "--model",
        "ollama/llama3",
        "--temperature",
        "0.25",
        "--max-turns",
        "3",
        "--tool-timeout",
        "5sec",
        "--store",
        "memory",
        "--repetition-guard",
        "off",
        "--compaction-strategy",
        "sliding_window",
        "--proactive-threshold-pct",
        "0.5",
        "--agent",
        "planner",
        "--name",
        "cli-agent",
        "-vv",
        "hello",
    ])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the flagged turn must complete: {e}"))?;

    // -- Check
    assert_eq!(
        agnt_testkit::received_request_count(env.llm()).await,
        1,
        "the flagged turn must issue exactly one completion"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_missing_config_errors() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let cli = support::parse_cli(&["hello"])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .err()
        .ok_or("a missing config.toml must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("No configuration found"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_malformed_config_errors() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_config("this is not = valid toml [[[")?;
    let cli = support::parse_cli(&["hello"])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .err()
        .ok_or("a malformed config.toml must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("Failed to load config.toml"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_unknown_role_errors() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let cli = support::parse_cli(&["--model", "heavy", "hello"])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .err()
        .ok_or("an unknown model role must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("Unknown model role: 'heavy'"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_missing_persona_errors() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let cli = support::parse_cli(&["--agent", "does-not-exist", "hello"])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .err()
        .ok_or("a missing persona must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("Agent persona not found"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_without_prompt_reports_no_prompt() -> Result<()> {
    // -- Setup & Fixtures
    if support::tui_mode_selected() {
        // With no prompt and both stdin and stderr attached to a terminal the
        // CLI selects TUI mode and takes over the terminal. The test only runs
        // when stderr mode is selected.
        return Ok(());
    }
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let cli = support::parse_cli(&[])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, None, &cli.ui)
        .await
        .err()
        .ok_or("stderr mode without a prompt must fail")?;

    // -- Check
    assert!(
        err.to_string()
            .contains("No prompt provided for stderr mode"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_literal_model_spec_reaches_llm() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    agnt_testkit::mount_text_response(env.llm(), "literal model ok", None).await;
    let cli = support::parse_cli(&["--model", "ollama/llama3", "hello"])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the literal-model turn must complete: {e}"))?;

    // -- Check
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    assert_eq!(
        bodies.len(),
        1,
        "the turn must issue exactly one completion"
    );
    let body = bodies.first().ok_or("one request body")?;
    assert!(
        body.contains("llama3"),
        "the literal provider/model spec must reach the LLM; got: {body}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_cli_model_overrides_persona_model() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    write_persona(
        &env,
        "cov-model",
        "---\nmodel: ollama/qwen3\n---\nYou are a coverage persona.\n",
    )?;
    agnt_testkit::mount_text_response(env.llm(), "cli model wins", None).await;
    let cli = support::parse_cli(&["--agent", "cov-model", "--model", "ollama/llama3", "hello"])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the persona turn must complete: {e}"))?;

    // -- Check
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    assert_eq!(
        bodies.len(),
        1,
        "the turn must issue exactly one completion"
    );
    let body = bodies.first().ok_or("one request body")?;
    assert!(
        body.contains("llama3"),
        "the CLI --model value must win over the persona model; got: {body}"
    );
    assert!(
        !body.contains("qwen3"),
        "the persona model must not reach the LLM when --model is set; got: {body}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_persona_model_applies_without_cli_model() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    write_persona(
        &env,
        "cov-model",
        "---\nmodel: ollama/qwen3\n---\nYou are a coverage persona.\n",
    )?;
    agnt_testkit::mount_text_response(env.llm(), "persona model wins", None).await;
    let cli = support::parse_cli(&["--agent", "cov-model", "hello"])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the persona-model turn must complete: {e}"))?;

    // -- Check
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    assert_eq!(
        bodies.len(),
        1,
        "the turn must issue exactly one completion"
    );
    let body = bodies.first().ok_or("one request body")?;
    assert!(
        body.contains("qwen3"),
        "the persona literal model spec must reach the LLM when --model is absent; got: {body}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_name_flag_completes_turn() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    agnt_testkit::mount_text_response(env.llm(), "named agent ok", None).await;
    let cli = support::parse_cli(&["--name", "cov-sub-agent", "hello"])?;

    // -- Exec
    dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .map_err(|e| format!("the named-agent turn must complete: {e}"))?;

    // -- Check
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    assert_eq!(
        bodies.len(),
        1,
        "the named-agent turn must issue exactly one completion"
    );
    let body = bodies.first().ok_or("one request body")?;
    assert!(
        body.contains("hello"),
        "the completion request must carry the prompt; got: {body}"
    );
    Ok(())
}

// endregion: --- Tests
