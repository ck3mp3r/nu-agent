use crate::plugin::{AgentPlugin, config_or_default};
use agnt_core::config::{PluginConfig, TomlConfigError};
use nu_plugin::Plugin;

use std::sync::{Mutex, Once};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// The log lines every `CaptureLogger` record lands in.
static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A logger that keeps the records a test asserts on.
struct CaptureLogger;

impl log::Log for CaptureLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if let Ok(mut logs) = CAPTURED.lock() {
            logs.push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

/// Install [`CaptureLogger`] once per test binary and clear the buffer.
fn capture_logs() {
    static SETUP: Once = Once::new();
    SETUP.call_once(|| {
        let _ = log::set_boxed_logger(Box::new(CaptureLogger));
        log::set_max_level(log::LevelFilter::Warn);
    });
    if let Ok(mut logs) = CAPTURED.lock() {
        logs.clear();
    }
}

// endregion: --- Test Support

#[test]
fn plugin_has_version() {
    let plugin = AgentPlugin::default();
    let version = plugin.version();
    assert!(!version.is_empty(), "Plugin version should not be empty");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn config_or_default_warns_before_falling_back() -> Result<()> {
    // -- Setup & Fixtures
    capture_logs();
    let broken = TomlConfigError::Io(std::io::Error::other("broken config"));

    // -- Exec
    let config = config_or_default(Err(broken));

    // -- Check: the derived default
    assert_eq!(config, PluginConfig::default(), "a failed load falls back");

    // -- Check: the failure is logged, so a user sees why [paths] was ignored
    let logged = CAPTURED.lock().map_err(|e| format!("{e:?}"))?.clone();
    assert!(
        logged
            .iter()
            .any(|line| line.contains("Failed to load config.toml")),
        "the fallback must log its reason, got: {logged:?}"
    );
    Ok(())
}

#[test]
fn config_or_default_keeps_a_loaded_config() -> Result<()> {
    // -- Setup & Fixtures
    let loaded = PluginConfig {
        theme: Some("catppuccin-latte".to_string()),
        ..PluginConfig::default()
    };

    // -- Exec
    let config = config_or_default(Ok(loaded.clone()));

    // -- Check
    assert_eq!(config, loaded);
    Ok(())
}
