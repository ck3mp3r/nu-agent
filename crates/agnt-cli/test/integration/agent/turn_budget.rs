//! Tool-call budget tests for `dispatch::run_agent`.
//!
//! The tool-call budget (`max_tool_turns`) is observable end-to-end: a mock
//! that answers every request with a tool call makes the agent issue exactly
//! one completion per tool-call round, so the request count is the budget times
//! the max-turns steering retries (the retry re-runs the turn once with a fresh
//! budget). The tests drive the real precedence chain — CLI flag > persona front
//! matter > pipeline-mode default.

use super::*;

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_persona_max_tool_turns_bounds_tool_loop() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    write_persona(
        &env,
        "cov-turns",
        "---\nmax_tool_turns: 9\n---\nYou are a coverage persona.\n",
    )?;
    let target = write_read_target(&env)?;
    mount_advancing_read_tool(&env, &target).await?;
    let cli = support::parse_cli(&[
        "--agent",
        "cov-turns",
        "--repetition-guard",
        "off",
        "keep reading",
    ])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .err()
        .ok_or("a mock that never stops calling tools must exhaust the budget")?;

    // -- Check
    assert!(
        err.to_string().contains("Max turns"),
        "the turn must fail on the exhausted tool-call budget; got: {err}"
    );
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    // The budget bounds the tool-call rounds; the max-turns steering retry
    // re-runs the turn once with a fresh budget.
    assert_eq!(
        bodies.len(),
        9 * (1 + usize::from(agnt_core::config::defaults::MAX_TURNS_FEEDBACK_RETRIES)),
        "the persona max_tool_turns value must bound the tool-call rounds"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_cli_max_turns_overrides_persona() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    write_persona(
        &env,
        "cov-turns",
        "---\nmax_tool_turns: 9\n---\nYou are a coverage persona.\n",
    )?;
    let target = write_read_target(&env)?;
    mount_advancing_read_tool(&env, &target).await?;
    let cli = support::parse_cli(&[
        "--agent",
        "cov-turns",
        "--max-turns",
        "3",
        "--repetition-guard",
        "off",
        "keep reading",
    ])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .err()
        .ok_or("a mock that never stops calling tools must exhaust the budget")?;

    // -- Check
    assert!(
        err.to_string().contains("Max turns"),
        "the turn must fail on the exhausted tool-call budget; got: {err}"
    );
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    // The CLI budget bounds the tool-call rounds; the max-turns steering retry
    // re-runs the turn once with a fresh budget.
    assert_eq!(
        bodies.len(),
        3 * (1 + usize::from(agnt_core::config::defaults::MAX_TURNS_FEEDBACK_RETRIES)),
        "the CLI --max-turns value must win over the persona max_tool_turns value"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_agent_run_agent_default_max_tool_turns_is_twenty() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let target = write_read_target(&env)?;
    mount_advancing_read_tool(&env, &target).await?;
    let cli = support::parse_cli(&["--repetition-guard", "off", "keep reading"])?;

    // -- Exec
    let err = dispatch::run_agent(&cli.args, cli.prompt.clone(), &cli.ui)
        .await
        .err()
        .ok_or("a mock that never stops calling tools must exhaust the budget")?;

    // -- Check
    assert!(
        err.to_string().contains("Max turns"),
        "the turn must fail on the exhausted tool-call budget; got: {err}"
    );
    let bodies = agnt_testkit::received_request_bodies(env.llm()).await;
    // The default budget bounds the tool-call rounds; the max-turns steering
    // retry re-runs the turn once with a fresh budget.
    assert_eq!(
        bodies.len(),
        20 * (1 + usize::from(agnt_core::config::defaults::MAX_TURNS_FEEDBACK_RETRIES)),
        "stderr mode must apply the pipeline-mode default budget of 20 tool-call rounds"
    );
    Ok(())
}

// endregion: --- Tests
