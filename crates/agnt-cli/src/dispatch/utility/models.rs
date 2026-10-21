//! `agent models` subcommand dispatch.

use agnt_core::config::{ModelSpec, ModelsCache};

use crate::ModelsCommand;
use crate::ModelsListArgs;
use crate::error::{Error, Result};

use super::table::print_table;

// region:    --- Functions

/// Dispatch an `agent models` subcommand.
pub fn run_models_command(command: &ModelsCommand) -> Result<()> {
    match command {
        ModelsCommand::Sync => run_models_sync(),
        ModelsCommand::List(args) => run_models_list(args),
    }
}

/// Fetch the latest model specs from models.dev and update the local cache.
fn run_models_sync() -> Result<()> {
    let cache = ModelsCache::fetch_and_store()
        .map_err(|e| Error::Custom(format!("Failed to sync models: {e}")))?;
    let provider_count = cache.providers.len();
    let model_count = cache.list_models(None).len();
    println!("Synced {provider_count} providers, {model_count} models");
    Ok(())
}

/// List available models from the local cache.
fn run_models_list(args: &ModelsListArgs) -> Result<()> {
    let cache = ModelsCache::load().map_err(|e| {
        Error::Custom(format!(
            "Failed to load models cache: {e}\nRun 'agent models sync' first."
        ))
    })?;

    let models = cache.list_models(args.provider.as_deref());
    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|(provider, model, spec)| {
            vec![
                (*provider).to_string(),
                (*model).to_string(),
                spec.limit.context.to_string(),
                spec.limit.output.to_string(),
                if spec.tool_call { "yes" } else { "no" }.to_string(),
                cost_cell(spec),
            ]
        })
        .collect();

    print_table(
        &["PROVIDER", "MODEL", "CONTEXT", "OUTPUT", "TOOLS", "COST"],
        &rows,
    );
    Ok(())
}

/// Format the input/output price per million tokens for a table cell.
fn cost_cell(spec: &ModelSpec) -> String {
    match &spec.cost {
        Some(cost) => format!("${:.2}/${:.2}", cost.input, cost.output),
        None => "-".to_string(),
    }
}

// endregion: --- Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/dispatch/utility/models.rs"]
mod models_test;

// endregion: --- Tests
