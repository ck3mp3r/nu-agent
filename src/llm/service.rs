use crate::config::Config;
use crate::llm::LlmResponse;
use crate::plugin::RuntimeCtx;
use nu_protocol::LabeledError;

pub async fn call_llm(
    runtime_ctx: &RuntimeCtx,
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    runtime_ctx.llm_runtime().call(config, prompt, tools).await
}
