//! Runtime boundary for LLM provider lifecycle.
//!
//! Responsibilities are strictly separated:
//! - [`key`] owns stable cache identity + auth fingerprinting
//! - [`cache`] owns concurrent get-or-create lifecycle
//! - [`provider_factory`] owns one-time config -> concrete provider selection
//! - [`provider_enum`] owns concrete provider execution dispatch
//!
//! Forbidden patterns:
//! - provider construction in `llm::call_llm`
//! - endpoint/model-family switching in `llm` orchestration layer

mod cache;
mod key;
mod provider_enum;
mod provider_factory;

pub use cache::ProviderCache;
pub use key::{ProviderKey, auth_fingerprint};

use crate::config::Config;
use crate::llm::LlmResponse;
use nu_protocol::LabeledError;
/// Single lifecycle entrypoint for provider acquisition and execution.
#[derive(Clone, Default)]
pub struct LlmRuntime {
    cache: ProviderCache,
}

impl LlmRuntime {
    pub fn new() -> Self {
        Self {
            cache: ProviderCache::new(),
        }
    }

    pub fn cache(&self) -> &ProviderCache {
        &self.cache
    }

    pub async fn call(
        &self,
        config: &Config,
        prompt: &str,
        tools: Vec<rig::completion::ToolDefinition>,
    ) -> Result<LlmResponse, LabeledError> {
        let key = ProviderKey::from_config(config);
        let cached = self
            .cache
            .get_or_create(key, || provider_factory::build_cached_provider(config))?;
        provider_enum::execute(cached.as_ref(), prompt, tools).await
    }
}

#[cfg(test)]
mod cache_test;

#[cfg(test)]
mod key_test;
