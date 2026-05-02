use crate::config::Config;
use crate::llm::LlmResponse;
use nu_protocol::LabeledError;

use super::{ProviderCache, ProviderKey, cached, factory};

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
            .get_or_create(key, || factory::build_cached_provider(config))?;
        cached::execute(cached.as_ref(), prompt, tools).await
    }
}
