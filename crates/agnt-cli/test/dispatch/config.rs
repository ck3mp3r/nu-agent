use super::*;

use clap::Parser;

use agnt_core::config::ConfigLoadError;
use agnt_core::config::StoreTypeConfig;
use agnt_core::config::toml_config::TomlConfigError;
use agnt_core::config::vault::VaultError;
use agnt_tty::policy::Verbosity;

use crate::Cli;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

fn parse_args(argv: &[&str]) -> Result<AgentArgs> {
    let mut full = vec!["agent"];
    full.extend_from_slice(argv);
    let cli = Cli::try_parse_from(full)?;
    Ok(cli.args)
}

// endregion: --- Test Support

#[test]
fn test_dispatch_config_apply_cli_flags_overrides_all_fields() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&[
        "--api-key",
        "sk-test",
        "--base-url",
        "https://example.test/v1",
        "--temperature",
        "0.7",
        "--max-context-tokens",
        "128000",
        "--max-output-tokens",
        "4096",
        "--max-turns",
        "12",
        "--repetition-guard",
        "off",
    ])?;
    let mut config = Config::default();

    // -- Exec
    apply_cli_flags_from_clap(&mut config, &args);

    // -- Check
    assert_eq!(config.api_key.as_deref(), Some("sk-test"));
    assert_eq!(config.base_url.as_deref(), Some("https://example.test/v1"));
    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_context_tokens, Some(128_000));
    assert_eq!(config.max_output_tokens, Some(4096));
    assert_eq!(config.max_tool_turns, Some(12));
    assert_eq!(config.repetition_guard, Some(false));
    Ok(())
}

#[test]
fn test_dispatch_config_apply_cli_flags_absent_leaves_config_untouched() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&[])?;
    let mut config = Config {
        api_key: Some("existing".to_string()),
        temperature: Some(0.1),
        max_tool_turns: Some(3),
        ..Config::default()
    };

    // -- Exec
    apply_cli_flags_from_clap(&mut config, &args);

    // -- Check
    assert_eq!(config.api_key.as_deref(), Some("existing"));
    assert_eq!(config.temperature, Some(0.1));
    assert_eq!(config.max_tool_turns, Some(3));
    assert_eq!(config.base_url, None);
    Ok(())
}

#[test]
fn test_dispatch_config_repetition_guard_values() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [("on", Some(true)), ("off", Some(false)), ("bogus", None)];

    // -- Exec & Check
    for (value, expected) in cases {
        let args = parse_args(&["--repetition-guard", value])?;
        let mut config = Config {
            repetition_guard: Some(true),
            ..Config::default()
        };
        apply_cli_flags_from_clap(&mut config, &args);
        assert_eq!(config.repetition_guard, expected, "value={value}");
    }
    Ok(())
}

#[test]
fn test_dispatch_config_max_context_tokens_overflow_is_ignored() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&["--max-context-tokens", "4294967296"])?;
    let mut config = Config::default();

    // -- Exec
    apply_cli_flags_from_clap(&mut config, &args);

    // -- Check
    assert_eq!(config.max_context_tokens, None);
    Ok(())
}

#[test]
fn test_dispatch_config_ui_policy_from_flags() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&[&str], bool, Verbosity)> = vec![
        (&[], false, Verbosity::Normal),
        (&["-q"], true, Verbosity::Quiet),
        (&["-v"], false, Verbosity::Verbose),
        (&["-vv"], false, Verbosity::VeryVerbose),
        (&["-vvv"], false, Verbosity::Trace),
        (&["-vvvv"], false, Verbosity::Trace),
        (&["-q", "-vvv"], true, Verbosity::Quiet),
    ];

    // -- Exec & Check
    for (flags, quiet, verbosity) in cases {
        let args = parse_args(flags)?;
        let policy = resolve_ui_policy_from_clap(&args);
        assert_eq!(policy.quiet, quiet, "flags={flags:?}");
        assert_eq!(policy.verbosity, verbosity, "flags={flags:?}");
    }
    Ok(())
}

#[test]
fn test_dispatch_config_theme_flag_takes_precedence() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig {
        theme: Some("catppuccin-latte".to_string()),
        ..PluginConfig::default()
    };

    // -- Exec
    let name = resolve_theme_from_clap(
        Some("catppuccin-frappe"),
        &plugin_config,
        std::path::Path::new("/nonexistent/theme.json"),
    )?;

    // -- Check
    assert_eq!(name, ThemeName::CatppuccinFrappe);
    Ok(())
}

#[test]
fn test_dispatch_config_theme_unknown_flag_errors() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig::default();

    // -- Exec
    let err = resolve_theme_from_clap(
        Some("solarized"),
        &plugin_config,
        std::path::Path::new("/nonexistent/theme.json"),
    )
    .err()
    .ok_or("should reject unknown theme")?;

    // -- Check
    assert!(err.to_string().contains("Unknown theme"));
    Ok(())
}

#[test]
fn test_dispatch_config_theme_from_config() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig {
        theme: Some("catppuccin-macchiato".to_string()),
        ..PluginConfig::default()
    };

    // -- Exec
    let name = resolve_theme_from_clap(
        None,
        &plugin_config,
        std::path::Path::new("/nonexistent/theme.json"),
    )?;

    // -- Check
    assert_eq!(name, ThemeName::CatppuccinMacchiato);
    Ok(())
}

#[test]
fn test_dispatch_config_compaction_flags() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&[
        "--compaction-strategy",
        "sliding_window",
        "--proactive-threshold-pct",
        "0.5",
    ])?;

    // -- Exec
    let config = extract_compaction_flags_from_clap(&args)?;

    // -- Check
    assert_eq!(config.strategy, Some(CompactionStrategy::SlidingSummary));
    assert_eq!(config.proactive_threshold_pct, Some(0.5));
    Ok(())
}

#[test]
fn test_dispatch_config_compaction_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&[])?;

    // -- Exec
    let config = extract_compaction_flags_from_clap(&args)?;

    // -- Check
    assert_eq!(config.strategy, None);
    assert_eq!(config.proactive_threshold_pct, None);
    Ok(())
}

#[test]
fn test_dispatch_config_compaction_invalid_strategy_errors() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&["--compaction-strategy", "bogus"])?;

    // -- Exec
    let err = extract_compaction_flags_from_clap(&args)
        .err()
        .ok_or("should reject unknown strategy")?;

    // -- Check
    assert!(err.to_string().contains("Invalid --compaction-strategy"));
    Ok(())
}

#[test]
fn test_dispatch_config_store_type_from_flags() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (None, StoreType::Sqlite),
        (Some("jsonl"), StoreType::Jsonl),
        (Some("memory"), StoreType::Memory),
        (Some("SQLITE"), StoreType::Sqlite),
    ];

    // -- Exec & Check
    for (value, expected) in cases {
        let args = match value {
            Some(v) => parse_args(&["--store", v])?,
            None => parse_args(&[])?,
        };
        assert_eq!(
            resolve_store_type_from_clap(&args, None)?,
            expected,
            "value={value:?}"
        );
    }

    let args = parse_args(&["--store", "bogus"])?;
    let err = resolve_store_type_from_clap(&args, None)
        .err()
        .ok_or("should reject unknown store")?;
    assert!(err.to_string().contains("Invalid --store value"));
    Ok(())
}

#[test]
fn test_dispatch_config_store_type_flag_wins_over_config() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&["--store", "jsonl"])?;

    // -- Exec
    let store_type = resolve_store_type_from_clap(&args, Some(StoreType::Memory))?;

    // -- Check
    assert_eq!(store_type, StoreType::Jsonl);
    Ok(())
}

#[test]
fn test_dispatch_config_store_type_config_value_when_flag_absent() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&[])?;

    // -- Exec
    let store_type = resolve_store_type_from_clap(&args, Some(StoreType::Jsonl))?;

    // -- Check
    assert_eq!(store_type, StoreType::Jsonl);
    Ok(())
}

#[test]
fn test_dispatch_config_store_type_default_when_both_absent() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&[])?;

    // -- Exec
    let store_type = resolve_store_type_from_clap(&args, None)?;

    // -- Check
    assert_eq!(store_type, StoreType::default());
    assert_eq!(store_type, StoreType::Sqlite);
    Ok(())
}

#[test]
fn test_dispatch_config_store_type_invalid_flag_errors_with_config_value() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&["--store", "bogus"])?;

    // -- Exec
    let err = resolve_store_type_from_clap(&args, Some(StoreType::Jsonl))
        .err()
        .ok_or("should reject unknown store")?;

    // -- Check
    assert!(err.to_string().contains("Invalid --store value"));
    Ok(())
}

#[test]
fn test_dispatch_config_tool_timeout_default() -> Result<()> {
    // -- Setup & Fixtures
    let args = parse_args(&[])?;

    // -- Exec
    let timeout = extract_tool_timeout_from_clap(&args)?;

    // -- Check
    assert_eq!(timeout, Duration::from_secs(30));
    Ok(())
}

#[test]
fn test_dispatch_config_tool_timeout_units() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("500ms", Duration::from_millis(500)),
        ("30sec", Duration::from_secs(30)),
        ("2min", Duration::from_secs(120)),
        ("1h", Duration::from_secs(3600)),
        ("1d", Duration::from_secs(86_400)),
        ("1w", Duration::from_secs(604_800)),
        ("10", Duration::from_secs(10)),
        ("1.5s", Duration::from_millis(1500)),
        ("1000us", Duration::from_millis(1)),
        ("1000000000ns", Duration::from_secs(1)),
    ];

    // -- Exec & Check
    for (input, expected) in cases {
        let arg = format!("--tool-timeout={input}");
        let args = parse_args(&[&arg])?;
        assert_eq!(
            extract_tool_timeout_from_clap(&args)?,
            expected,
            "input={input}"
        );
    }
    Ok(())
}

#[test]
fn test_dispatch_config_tool_timeout_invalid_errors() -> Result<()> {
    // -- Setup & Fixtures
    let cases = ["abc", "5fortnight", "-5s"];

    // -- Exec & Check
    for input in cases {
        let arg = format!("--tool-timeout={input}");
        let args = parse_args(&[&arg])?;
        let err = extract_tool_timeout_from_clap(&args)
            .err()
            .ok_or("should reject invalid duration")?;
        assert!(err.to_string().contains("duration"), "input={input}");
    }
    Ok(())
}

#[test]
fn test_dispatch_config_map_config_load_error_messages() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(ConfigLoadError, &str)> = vec![
        (
            ConfigLoadError::NotFound(std::path::PathBuf::from("/tmp/config.toml")),
            "No configuration found",
        ),
        (
            ConfigLoadError::Toml(TomlConfigError::InvalidApiKey("bad key".to_string())),
            "Failed to load config.toml: bad key",
        ),
        (
            ConfigLoadError::UnknownRole("heavy".to_string()),
            "Unknown model role: 'heavy'",
        ),
        (ConfigLoadError::NoDefaultModel, "No model configured"),
        (
            ConfigLoadError::Resolve("boom".to_string()),
            "Failed to resolve model: boom",
        ),
        (
            ConfigLoadError::Vault(VaultError::NoBackend),
            "No vault backend available",
        ),
    ];

    // -- Exec & Check
    for (err, needle) in cases {
        let mapped = map_config_load_error(err);
        assert!(mapped.to_string().contains(needle), "needle={needle}");
    }
    Ok(())
}

#[test]
fn test_dispatch_config_session_store_type_from_config_load_invalid_api_key() -> Result<()> {
    // -- Setup & Fixtures
    let loaded = Err(TomlConfigError::InvalidApiKey("bad key".to_string()));

    // -- Exec
    let store_type = session_store_type_from_config_load(loaded);

    // -- Check
    assert_eq!(store_type, None);
    Ok(())
}

#[test]
fn test_dispatch_config_session_store_type_from_config_load_io_error() -> Result<()> {
    // -- Setup & Fixtures
    let loaded = Err(TomlConfigError::Io(std::io::Error::other("disk on fire")));

    // -- Exec
    let store_type = session_store_type_from_config_load(loaded);

    // -- Check
    assert_eq!(store_type, None);
    Ok(())
}

#[test]
fn test_dispatch_config_session_store_type_from_config_load_with_store() -> Result<()> {
    // -- Setup & Fixtures
    let loaded = Ok(PluginConfig {
        session_store: Some(StoreTypeConfig {
            store_type: StoreType::Jsonl,
            path: None,
        }),
        ..Default::default()
    });

    // -- Exec
    let store_type = session_store_type_from_config_load(loaded);

    // -- Check
    assert_eq!(store_type, Some(StoreType::Jsonl));
    Ok(())
}

#[test]
fn test_dispatch_config_session_store_type_from_config_load_default() -> Result<()> {
    // -- Setup & Fixtures
    let loaded = Ok(PluginConfig::default());

    // -- Exec
    let store_type = session_store_type_from_config_load(loaded);

    // -- Check
    assert_eq!(store_type, None);
    Ok(())
}
