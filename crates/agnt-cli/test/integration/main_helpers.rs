//! Integration tests for the `main.rs` prompt and logging helpers.
//!
//! `main()` itself cannot be called from a test, but `resolve_prompt`,
//! `init_logging`, and `environment_level` are reachable and cover the CLI's
//! startup wiring.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_integration_main_resolve_prompt_returns_positional() {
    // -- Exec
    let prompt = resolve_prompt(Some("hello".to_string()));

    // -- Check
    assert_eq!(prompt.as_deref(), Some("hello"));
}

#[test]
fn test_integration_main_resolve_prompt_without_positional_uses_stdin() {
    // -- Setup & Fixtures
    // The test harness inherits stdin. A terminal or an empty pipe yields no
    // prompt; a pipe that carried text yields that text.

    // -- Exec
    let prompt = resolve_prompt(None);

    // -- Check
    if let Some(text) = prompt {
        assert!(
            !text.trim().is_empty(),
            "a piped prompt must be non-empty after trimming"
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_main_init_logging_targets_state_dir() -> Result<()> {
    // -- Setup & Fixtures
    // No log flags: the file sink is the default, with no configuration.
    let env = support::TestEnv::new().await?;
    let cli = support::parse_cli(&["hello"])?;

    // -- Exec
    init_logging(&cli.args, &cli.command);

    // -- Check: the instance's own file, named by the pid `LogIdentity::new`
    // read inside init_logging.
    let relative = format!(
        "{}/logs/agent-{}.log",
        agnt_runtime::utils::paths::APP_DIR_NAME,
        std::process::id()
    );
    let log_path = env.state_home_path(&relative);
    assert!(
        log_path.exists(),
        "init_logging must open the instance's log file at {}",
        log_path.display()
    );
    Ok(())
}

#[test]
fn test_integration_main_log_role_for_maps_every_command() -> Result<()> {
    use agnt_runtime::logging::LogRole;

    // -- Setup & Fixtures
    let cases = [
        (None, LogRole::Tui),
        (Some(Command::Acp(AcpArgs { agent: None })), LogRole::Acp),
        (
            Some(Command::A2a(A2aArgs {
                port: 0,
                mesh_key: None,
                ui: UiArgs { theme: None },
            })),
            LogRole::A2a,
        ),
        (
            Some(Command::Config(ConfigCommand::Init(ConfigInitArgs {
                force: false,
            }))),
            LogRole::Run,
        ),
        (Some(Command::Models(ModelsCommand::Sync)), LogRole::Run),
        (
            Some(Command::Provider(ProviderCommand::Auth(
                ProviderAuthCommand::Status,
            ))),
            LogRole::Run,
        ),
        (
            Some(Command::Mcp(McpCommand::Auth(McpAuthCommand::Status))),
            LogRole::Run,
        ),
        (Some(Command::Session(SessionCommand::List)), LogRole::Run),
    ];

    // -- Exec & Check
    for (command, expected) in cases {
        assert_eq!(log_role_for(&command), expected, "{command:?}");
    }
    Ok(())
}

// endregion: --- Tests
