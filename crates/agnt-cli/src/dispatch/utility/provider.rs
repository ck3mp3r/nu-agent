//! `agent provider` subcommand dispatch.

use agnt_core::config::copilot_tokens::{
    read_copilot_token_record, verify_copilot_login_persistence,
};
use agnt_core::config::vault::{CredentialType, Vault};
use agnt_core::types::default_token_dir;

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

    let vault = Vault::auto_detect().map_err(|e| Error::Custom(e.to_string()))?;
    vault
        .store_api_key(name, &key)
        .map_err(|e| Error::Custom(format!("Failed to store API key: {e}")))?;

    println!("Stored API key for '{name}'");
    Ok(())
}

/// Run the GitHub Copilot OAuth device-code flow.
async fn run_copilot_login() -> Result<()> {
    use agnt_core::types::{AuthSource, Authenticator, CopilotConfig, DeviceCodeHandler};

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

    let token_dir = token_dir.ok_or_else(|| {
        Error::Custom(
            "GitHub Copilot credential directory unavailable: set XDG_CONFIG_HOME or HOME."
                .to_string(),
        )
    })?;
    verify_copilot_login_persistence(&token_dir).map_err(Error::Custom)?;

    let api_key_path = token_dir.join("api-key.json");
    let access_token_path = token_dir.join("access-token");
    println!(
        "Successfully authenticated with GitHub Copilot\n  api-key.json: {}\n  access-token: {}",
        api_key_path.display(),
        access_token_path.display()
    );
    Ok(())
}

/// Clear stored credentials for an LLM provider.
fn run_provider_logout(args: &ProviderLogoutArgs) -> Result<()> {
    let name = args.name.as_str();

    // github-copilot stores its tokens in files, not the vault.
    if (name == "github-copilot" || name == "copilot")
        && let Some(dir) = default_token_dir()
        && read_copilot_token_record(&dir)
            .map_err(Error::Custom)?
            .is_some()
    {
        remove_copilot_token_files(&dir).map_err(Error::Custom)?;
        println!("Removed credentials for '{name}'");
        return Ok(());
    }

    let vault = Vault::auto_detect().map_err(|e| Error::Custom(e.to_string()))?;
    let removed = vault
        .remove_provider(name)
        .map_err(|e| Error::Custom(format!("Failed to remove credentials: {e}")))?;

    if removed {
        println!("Removed credentials for '{name}'");
    } else {
        println!("No stored credentials for '{name}'");
    }
    Ok(())
}

/// Show stored credentials for all LLM providers.
fn run_provider_status() -> Result<()> {
    let vault = Vault::auto_detect().map_err(|e| Error::Custom(e.to_string()))?;
    let entries = vault
        .list_providers()
        .map_err(|e| Error::Custom(format!("Failed to list credentials: {e}")))?;

    let mut rows: Vec<Vec<String>> = entries
        .iter()
        .map(|entry| {
            let (cred_type, expiry) = match entry.credential_type {
                CredentialType::ApiKey => ("api_key".to_string(), "-".to_string()),
                CredentialType::OAuth => ("oauth".to_string(), format_expiry(entry.expires_at)),
            };
            vec![entry.name.clone(), cred_type, expiry]
        })
        .collect();

    // github-copilot stores its tokens in files, not the vault.
    if let Some(dir) = default_token_dir()
        && let Some(record) = read_copilot_token_record(&dir).map_err(Error::Custom)?
    {
        rows.push(vec![
            "github-copilot".to_string(),
            "oauth".to_string(),
            format_expiry(record.expires_at.map(|t| t as u64)),
        ]);
    }

    if rows.is_empty() {
        println!("No stored provider credentials.");
        return Ok(());
    }

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

/// Delete the two Copilot token files, leaving the directory in place.
fn remove_copilot_token_files(dir: &std::path::Path) -> core::result::Result<(), String> {
    for file in ["access-token", "api-key.json"] {
        let path = dir.join(file);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Failed to remove {}: {e}", path.display())),
        }
    }
    Ok(())
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/dispatch/utility/provider.rs"]
mod provider_test;

// endregion: --- Tests
