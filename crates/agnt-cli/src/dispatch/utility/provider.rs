//! `agent provider` subcommand dispatch.

use agnt_core::config::vault::{CredentialType, Vault};

use crate::ProviderAuthCommand;
use crate::ProviderCommand;
use crate::ProviderLoginArgs;
use crate::ProviderLogoutArgs;
use crate::error::{Error, Result};

use super::table::print_table;

// region:    --- Functions

/// Dispatch an `agent provider` subcommand.
pub async fn run_provider_command(command: &ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::Auth(auth) => run_provider_auth_command(auth).await,
    }
}

async fn run_provider_auth_command(command: &ProviderAuthCommand) -> Result<()> {
    match command {
        ProviderAuthCommand::Login(args) => run_provider_login(args).await,
        ProviderAuthCommand::Logout(args) => run_provider_logout(args),
        ProviderAuthCommand::Status => run_provider_status(),
    }
}

/// Authenticate with an LLM provider.
async fn run_provider_login(args: &ProviderLoginArgs) -> Result<()> {
    let name = args.name.as_str();

    // github-copilot uses the OAuth device-code flow.
    if name == "github-copilot" || name == "copilot" {
        return run_copilot_login().await;
    }

    let key = match &args.api_key {
        Some(key) => key.clone(),
        None => read_stdin_key().ok_or_else(|| {
            Error::Custom(
                "API key required. Use --api-key <key> or pipe the key: echo \"sk-...\" | agent provider auth login <name>"
                    .to_string(),
            )
        })?,
    };

    let vault = Vault::auto_detect();
    vault
        .store_api_key(name, &key)
        .map_err(|e| Error::Custom(format!("Failed to store API key: {e}")))?;

    println!("Stored API key for '{name}'");
    Ok(())
}

/// Run the GitHub Copilot OAuth device-code flow.
async fn run_copilot_login() -> Result<()> {
    use agnt_core::types::{
        AuthSource, Authenticator, CopilotConfig, DeviceCodeHandler, default_token_dir,
    };

    let handler = DeviceCodeHandler::new(|prompt| {
        eprintln!(
            "Sign in with GitHub Copilot:\n  1) Visit: {}\n  2) Enter code: {}",
            prompt.verification_uri, prompt.user_code
        );
    });
    let token_dir = default_token_dir();
    let authenticator = Authenticator::new(
        AuthSource::OAuth,
        token_dir.as_ref().map(|dir| dir.join("access-token")),
        token_dir.as_ref().map(|dir| dir.join("api-key.json")),
        handler,
        true,
    );

    CopilotConfig::new("")
        .client()
        .authenticate(&authenticator)
        .await
        .map_err(|e| Error::Custom(format!("Authentication failed: {e}")))?;

    println!("Successfully authenticated with GitHub Copilot");
    Ok(())
}

/// Clear stored credentials for an LLM provider.
fn run_provider_logout(args: &ProviderLogoutArgs) -> Result<()> {
    let vault = Vault::auto_detect();
    let removed = vault
        .remove_provider(&args.name)
        .map_err(|e| Error::Custom(format!("Failed to remove credentials: {e}")))?;

    if removed {
        println!("Removed credentials for '{}'", args.name);
    } else {
        println!("No stored credentials for '{}'", args.name);
    }
    Ok(())
}

/// Show stored credentials for all LLM providers.
fn run_provider_status() -> Result<()> {
    let vault = Vault::auto_detect();
    let entries = vault
        .list_providers()
        .map_err(|e| Error::Custom(format!("Failed to list credentials: {e}")))?;

    if entries.is_empty() {
        println!("No stored provider credentials.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|entry| {
            let (cred_type, expiry) = match entry.credential_type {
                CredentialType::ApiKey => ("api_key".to_string(), "-".to_string()),
                CredentialType::OAuth => ("oauth".to_string(), format_expiry(entry.expires_at)),
            };
            vec![entry.name.clone(), cred_type, expiry]
        })
        .collect();

    print_table(&["PROVIDER", "TYPE", "EXPIRES AT"], &rows);
    Ok(())
}

// endregion: --- Functions

// region:    --- Support

/// Read a non-empty API key from piped stdin, or `None` when stdin is a terminal.
fn read_stdin_key() -> Option<String> {
    use std::io::{IsTerminal, Read};

    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer).ok()?;
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Format a unix-seconds expiry as RFC 3339, or `-` when absent.
fn format_expiry(expires_at: Option<u64>) -> String {
    let Some(seconds) = expires_at else {
        return "-".to_string();
    };
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds as i64, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| seconds.to_string())
}

// endregion: --- Support
