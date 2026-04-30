//! Runtime boundary for LLM provider lifecycle.
//!
//! Responsibilities are strictly separated:
//! - [`providers::key`] owns stable cache identity + auth fingerprinting
//! - [`providers::cache`] owns concurrent get-or-create lifecycle
//! - [`providers::factory`] owns one-time config -> concrete provider selection
//! - [`providers::cached`] owns concrete provider execution dispatch
//!
//! Forbidden patterns:
//! - provider construction in `llm::call_llm`
//! - endpoint/model-family switching in `llm` orchestration layer

mod providers;

pub use providers::{ProviderCache, ProviderKey, auth_fingerprint};

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
            .get_or_create(key, || providers::factory::build_cached_provider(config))?;
        providers::cached::execute(cached.as_ref(), prompt, tools).await
    }
}
