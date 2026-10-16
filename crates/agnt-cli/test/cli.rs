use super::*;

use clap::Parser;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_cli_parses_a2a_args() -> Result<()> {
    // -- Exec
    let cli = Cli::try_parse_from([
        "agent",
        "a2a",
        "--port",
        "8080",
        "--mesh-key",
        "mesh-1",
        "--theme",
        "catppuccin-latte",
    ])?;

    // -- Check
    match cli.command {
        Some(Command::A2a(a2a)) => {
            assert_eq!(a2a.port, 8080);
            assert_eq!(a2a.mesh_key.as_deref(), Some("mesh-1"));
            assert_eq!(a2a.ui.theme.as_deref(), Some("catppuccin-latte"));
        }
        _ => return Err("expected a2a command".into()),
    }
    Ok(())
}

#[test]
fn test_cli_a2a_defaults() -> Result<()> {
    // -- Exec
    let cli = Cli::try_parse_from(["agent", "a2a"])?;

    // -- Check
    match cli.command {
        Some(Command::A2a(a2a)) => {
            assert_eq!(a2a.port, 0);
            assert!(a2a.mesh_key.is_none());
            assert!(a2a.ui.theme.is_none());
        }
        _ => return Err("expected a2a command".into()),
    }
    Ok(())
}

#[test]
fn test_cli_parses_global_flags_with_prompt() -> Result<()> {
    // -- Exec
    let cli = Cli::try_parse_from([
        "agent",
        "-m",
        "openai/gpt-4o",
        "--temperature",
        "0.3",
        "-vv",
        "hello world",
    ])?;

    // -- Check
    assert!(cli.command.is_none());
    assert_eq!(cli.prompt.as_deref(), Some("hello world"));
    assert_eq!(cli.args.model.as_deref(), Some("openai/gpt-4o"));
    assert_eq!(cli.args.temperature, Some(0.3));
    assert_eq!(cli.args.verbose, 2);
    Ok(())
}

#[test]
fn test_cli_parses_utility_subcommands() -> Result<()> {
    // -- Exec & Check
    let cli = Cli::try_parse_from(["agent", "config", "init", "--force"])?;
    assert!(matches!(
        cli.command,
        Some(Command::Config(ConfigCommand::Init(ConfigInitArgs {
            force: true
        })))
    ));

    let cli = Cli::try_parse_from(["agent", "models", "list", "--provider", "openai"])?;
    match cli.command {
        Some(Command::Models(ModelsCommand::List(args))) => {
            assert_eq!(args.provider.as_deref(), Some("openai"));
        }
        _ => return Err("expected models list".into()),
    }

    let cli = Cli::try_parse_from(["agent", "session", "clear", "abc123"])?;
    match cli.command {
        Some(Command::Session(SessionCommand::Clear(args))) => assert_eq!(args.id, "abc123"),
        _ => return Err("expected session clear".into()),
    }

    let cli = Cli::try_parse_from([
        "agent",
        "provider",
        "auth",
        "login",
        "openai",
        "--api-key",
        "sk-1",
    ])?;
    match cli.command {
        Some(Command::Provider(ProviderCommand::Auth(ProviderAuthCommand::Login(args)))) => {
            assert_eq!(args.name, "openai");
            assert_eq!(args.api_key.as_deref(), Some("sk-1"));
        }
        _ => return Err("expected provider auth login".into()),
    }

    let cli = Cli::try_parse_from(["agent", "mcp", "auth", "logout", "srv"])?;
    match cli.command {
        Some(Command::Mcp(McpCommand::Auth(McpAuthCommand::Logout(args)))) => {
            assert_eq!(args.server, "srv");
        }
        _ => return Err("expected mcp auth logout".into()),
    }

    let cli = Cli::try_parse_from(["agent", "acp"])?;
    match cli.command {
        Some(Command::Acp(args)) => assert!(
            args.agent.is_none(),
            "acp without --agent must leave the agent unset"
        ),
        _ => return Err("expected acp".into()),
    }

    let cli = Cli::try_parse_from(["agent", "acp", "--agent", "dev"])?;
    match cli.command {
        Some(Command::Acp(args)) => assert_eq!(args.agent.as_deref(), Some("dev")),
        _ => return Err("expected acp".into()),
    }
    Ok(())
}

#[test]
fn test_cli_resolve_prompt_prefers_positional() {
    // -- Exec
    let prompt = resolve_prompt(Some("hi".to_string()));

    // -- Check
    assert_eq!(prompt.as_deref(), Some("hi"));
}
