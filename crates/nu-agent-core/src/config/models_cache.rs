use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::utils::xdg::{self, XdgError};

/// Local cache of the models.dev database.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ModelsCache {
    #[serde(flatten)]
    pub providers: HashMap<String, ProviderSpec>,
}

/// Specification for a single model provider.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProviderSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub env: Vec<String>,
    pub api: Option<String>,
    #[serde(default)]
    pub models: HashMap<String, ModelSpec>,
}

/// Specification for a single model.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tool_call: bool,
    pub limit: ModelLimit,
    pub cost: Option<ModelCost>,
    pub modalities: Option<Modalities>,
}

/// Token limits for a model.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ModelLimit {
    pub context: u32,
    pub output: u32,
}

/// Per-token cost for a model.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
}

/// Input/output modalities supported by a model.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// Errors that can occur during models cache operations.
#[derive(Debug)]
pub enum ModelsCacheError {
    NotFound(PathBuf),
    Parse(serde_json::Error),
    Io(std::io::Error),
    Http(String),
    Xdg(XdgError),
}

impl std::fmt::Display for ModelsCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(p) => write!(f, "models cache not found at {}", p.display()),
            Self::Parse(e) => write!(f, "failed to parse models cache: {e}"),
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Http(e) => write!(f, "HTTP error fetching models cache: {e}"),
            Self::Xdg(e) => write!(f, "failed to resolve data directory: {e}"),
        }
    }
}

impl std::error::Error for ModelsCacheError {}

impl ModelsCache {
    fn path() -> Result<PathBuf, ModelsCacheError> {
        let dir = xdg::data_dir().map_err(ModelsCacheError::Xdg)?;
        Ok(dir.join("nu-agent").join("models.json"))
    }

    /// Load the models cache from disk.
    pub fn load() -> Result<Self, ModelsCacheError> {
        let path = Self::path()?;
        if !path.exists() {
            return Err(ModelsCacheError::NotFound(path));
        }
        let contents = std::fs::read_to_string(&path).map_err(ModelsCacheError::Io)?;
        let cache: ModelsCache =
            serde_json::from_str(&contents).map_err(ModelsCacheError::Parse)?;
        Ok(cache)
    }

    /// Fetch the latest models.dev database and store it locally.
    pub fn fetch_and_store() -> Result<Self, ModelsCacheError> {
        let client = reqwest::blocking::Client::new();
        let response = client
            .get("https://models.dev/api.json")
            .send()
            .map_err(|e| ModelsCacheError::Http(e.to_string()))?
            .text()
            .map_err(|e| ModelsCacheError::Http(e.to_string()))?;
        let cache: ModelsCache =
            serde_json::from_str(&response).map_err(ModelsCacheError::Parse)?;
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ModelsCacheError::Io)?;
        }
        let pretty = serde_json::to_string_pretty(&cache).map_err(ModelsCacheError::Parse)?;
        std::fs::write(&path, pretty).map_err(ModelsCacheError::Io)?;
        Ok(cache)
    }

    /// Get the spec for a specific provider/model combination.
    pub fn get_spec(&self, provider: &str, model: &str) -> Option<&ModelSpec> {
        self.providers.get(provider)?.models.get(model)
    }

    /// List all models, optionally filtered by provider.
    pub fn list_models(&self, provider: Option<&str>) -> Vec<(&str, &str, &ModelSpec)> {
        let mut result = Vec::new();
        if let Some(p) = provider {
            if let Some(spec) = self.providers.get(p) {
                for (model_id, model_spec) in &spec.models {
                    result.push((spec.id.as_str(), model_id.as_str(), model_spec));
                }
            }
        } else {
            for (provider_id, provider_spec) in &self.providers {
                for (model_id, model_spec) in &provider_spec.models {
                    result.push((provider_id.as_str(), model_id.as_str(), model_spec));
                }
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "models_cache_test.rs"]
mod models_cache_test;
