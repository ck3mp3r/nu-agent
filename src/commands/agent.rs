use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Signature, Type, Value};

use crate::{
    AgentPlugin,
    config::{Config, PluginConfig},
};

/// Trait abstracting the engine interface functionality needed for config resolution.
///
/// This allows us to mock the EngineInterface for testing without needing
/// a real Nushell engine instance.
pub trait EngineConfigInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError>;
}

impl EngineConfigInterface for EngineInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError> {
        // Convert ShellError to LabeledError
        self.get_plugin_config()
            .map_err(|e| LabeledError::new(format!("Failed to get plugin config: {}", e)))
    }
}

/// Extract prompt string from input Value.
///
/// Supports two input formats:
/// 1. String input: "prompt text"
/// 2. Record input: {prompt: "prompt text", context?: "...", model?: "...", tools?: [...]}
///
/// # Arguments
/// * `input` - The input Value, expected to be a String or Record with 'prompt' field
///
/// # Returns
/// The prompt string, or error if input is invalid
///
/// # Errors
/// - Input is not a String or Record
/// - Record input missing 'prompt' field
/// - Prompt is empty or contains only whitespace
pub fn extract_prompt_from_input(input: &Value) -> Result<String, LabeledError> {
    // Try to extract as string first (original behavior)
    if let Ok(prompt_str) = input.as_str() {
        // Check for empty string
        if prompt_str.trim().is_empty() {
            return Err(
                LabeledError::new("Empty prompt")
                    .with_label("Prompt cannot be empty", input.span())
            );
        }
        return Ok(prompt_str.to_string());
    }

    // Try to extract as record
    if let Ok(record) = input.as_record() {
        // Look for 'prompt' field
        let prompt_value = record.get("prompt").ok_or_else(|| {
            LabeledError::new("Missing required field")
                .with_label("Record input must have 'prompt' field", input.span())
        })?;

        // Extract string from prompt field
        let prompt_str = prompt_value.as_str().map_err(|_| {
            LabeledError::new("Invalid prompt type")
                .with_label("'prompt' field must be a string", prompt_value.span())
        })?;

        // Check for empty string
        if prompt_str.trim().is_empty() {
            return Err(
                LabeledError::new("Empty prompt")
                    .with_label("Prompt cannot be empty", prompt_value.span())
            );
        }

        return Ok(prompt_str.to_string());
    }

    // Neither string nor record - error
    Err(
        LabeledError::new("Invalid input type")
            .with_label("Expected a string prompt or record with 'prompt' field", input.span())
    )
}

/// Extract optional context string from input Value.
///
/// Supports two input formats:
/// 1. String input: Returns None (no context field available)
/// 2. Record input: Returns Some(context) if 'context' field exists, None otherwise
///
/// # Arguments
/// * `input` - The input Value
///
/// # Returns
/// Optional context string, or error if context field has invalid type
///
/// # Errors
/// - Context field exists but is not a string
pub fn extract_context_from_input(input: &Value) -> Result<Option<String>, LabeledError> {
    // String input has no context field
    if input.as_str().is_ok() {
        return Ok(None);
    }

    // Try to extract as record
    if let Ok(record) = input.as_record() {
        // Look for optional 'context' field
        if let Some(context_value) = record.get("context") {
            // Extract string from context field
            let context_str = context_value.as_str().map_err(|_| {
                LabeledError::new("Invalid context type")
                    .with_label("'context' field must be a string", context_value.span())
            })?;

            return Ok(Some(context_str.to_string()));
        }

        // No context field - that's OK
        return Ok(None);
    }

    // Neither string nor record - no context
    Ok(None)
}

/// Merge optional context with prompt for LLM call.
///
/// If context is provided and non-empty, prepends it to the prompt with clear separation.
/// Empty or whitespace-only context is treated as None.
///
/// # Arguments
/// * `prompt` - The main prompt text
/// * `context` - Optional context to prepend to the prompt
///
/// # Returns
/// Combined prompt string with context prepended if provided
pub fn merge_prompt_with_context(prompt: &str, context: Option<&str>) -> String {
    match context {
        Some(ctx) if !ctx.trim().is_empty() => {
            format!("{}\n\n---\n\n{}", ctx, prompt)
        }
        _ => prompt.to_string(),
    }
}

pub struct Agent;

impl SimplePluginCommand for Agent {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Send a prompt to an AI agent and get a structured response"
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .input_output_types(vec![
                (Type::String, Type::Record(vec![].into())),
                (Type::Record(vec![].into()), Type::Record(vec![].into())),
            ])
            .category(Category::Experimental)
            .named(
                "provider",
                nu_protocol::SyntaxShape::String,
                "[DEPRECATED] LLM provider name - use --model with provider/model format instead",
                Some('p'),
            )
            .named(
                "model",
                nu_protocol::SyntaxShape::String,
                "Model to use in provider/model format (e.g., 'openai/gpt-4', 'anthropic/claude-3-opus')",
                Some('m'),
            )
            .switch(
                "small",
                "Use the small/fast model configured in plugin config",
                Some('s'),
            )
            .named(
                "api-key",
                nu_protocol::SyntaxShape::String,
                "API key override for the provider",
                None,
            )
            .named(
                "base-url",
                nu_protocol::SyntaxShape::String,
                "Custom API endpoint URL",
                None,
            )
            .named(
                "temperature",
                nu_protocol::SyntaxShape::Number,
                "Sampling temperature (0.0 to 2.0)",
                None,
            )
            .named(
                "max-tokens",
                nu_protocol::SyntaxShape::Int,
                "Maximum tokens to generate",
                None,
            )
            .named(
                "max-context-tokens",
                nu_protocol::SyntaxShape::Int,
                "Maximum context window size (input + output)",
                None,
            )
            .named(
                "max-output-tokens",
                nu_protocol::SyntaxShape::Int,
                "Maximum output tokens",
                None,
            )
            .named(
                "max-turns",
                nu_protocol::SyntaxShape::Int,
                "Maximum tool calling turns",
                None,
            )
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        // Resolve configuration from all sources with proper precedence:
        // default < env < plugin < flags
        let config = resolve_config(engine, call)?;

        // Extract prompt from input
        let prompt = extract_prompt_from_input(input)?;

        // Extract optional context from input
        let context = extract_context_from_input(input)?;

        // Merge context into prompt for LLM call
        let merged_prompt = merge_prompt_with_context(&prompt, context.as_deref());

        // Call LLM (async operation - we need to block on it)
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| LabeledError::new(format!("Failed to create async runtime: {}", e)))?;

        let response = runtime
            .block_on(crate::llm::call_llm(&config, &merged_prompt))
            .map_err(|e| {
                LabeledError::new(format!("LLM call failed: {}", e.msg))
                    .with_label(e.msg, call.head)
            })?;

        // Format response as Nushell Value
        Ok(crate::llm::format_response(&response, &config, call.head))
    }
}

/// Extract configuration from command-line flags.
///
/// Reads flags from the EvaluatedCall and returns a Config with values for
/// provided flags and None for unprovided flags.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing command flags
///
/// # Returns
/// Config with values from flags or Config::default() fields for unprovided flags
pub fn extract_flag_config(call: &EvaluatedCall) -> Config {
    // Helper to safely extract string flag
    fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
    }

    // Helper to safely extract float flag
    fn get_float_flag(call: &EvaluatedCall, name: &str) -> Option<f64> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_float().ok())
    }

    // Helper to safely extract u32 flag (from i64, rejecting negatives)
    fn get_u32_flag(call: &EvaluatedCall, name: &str) -> Option<u32> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_int().ok())
            .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    }

    // Extract all flags
    let provider = get_string_flag(call, "provider").unwrap_or_default();
    let model = get_string_flag(call, "model").unwrap_or_default();
    let api_key = get_string_flag(call, "api-key");
    let base_url = get_string_flag(call, "base-url");
    let temperature = get_float_flag(call, "temperature");
    let max_tokens = get_u32_flag(call, "max-tokens");
    let max_context_tokens = get_u32_flag(call, "max-context-tokens");
    let max_output_tokens = get_u32_flag(call, "max-output-tokens");
    let max_tool_turns = get_u32_flag(call, "max-turns");

    Config {
        provider,
        provider_impl: None,
        model,
        api_key,
        base_url,
        temperature,
        max_tokens,
        max_context_tokens,
        max_output_tokens,
        max_tool_turns,
    }
}

/// Resolve configuration from all sources with proper precedence.
///
/// NEW Resolution pipeline:
/// 1. Parse PluginConfig from $env.config.plugins.agent (if present)
/// 2. Determine active model:
///    - If --model flag provided: use it (provider/model format)
///    - Else if --small flag provided: use small_model from PluginConfig
///    - Else use config.model (default)
/// 3. Call PluginConfig::resolve_model() to get base Config
/// 4. Merge with flag overrides (temperature, max_tokens, etc.)
/// 5. Validate and return
///
/// FALLBACK for backward compatibility:
/// - If plugin config doesn't have new structure (no "providers" field)
/// - Fall back to OLD Config::from_plugin_config() behavior
/// - Support old --provider and --model flags (separate)
///
/// # Arguments
/// * `engine` - Engine interface for accessing plugin config
/// * `call` - The EvaluatedCall containing command flags
///
/// # Returns
/// Fully resolved and validated Config, or error if validation fails
pub fn resolve_config<E: EngineConfigInterface>(
    engine: &E,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Step 1: Get plugin config value (if present)
    let plugin_config_opt = engine.get_plugin_config()?;

    // Step 2: Try NEW plugin config structure first
    if let Some(ref plugin_value) = plugin_config_opt {
        // Try to parse as NEW PluginConfig structure
        if let Ok(plugin_config) = PluginConfig::from_plugin_config(plugin_value) {
            // NEW FLOW: Use PluginConfig
            return resolve_with_new_config(plugin_config, call);
        }
        // If parsing failed, fall through to OLD flow
    }

    // Step 3: FALLBACK to OLD flow for backward compatibility
    resolve_with_old_config(plugin_config_opt, call)
}

/// NEW resolution flow using PluginConfig structure
fn resolve_with_new_config(
    plugin_config: PluginConfig,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Helper to get string flag
    fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
    }

    // Helper to get bool flag (switch)
    fn get_bool_flag(call: &EvaluatedCall, name: &str) -> bool {
        call.get_flag(name).ok().flatten().unwrap_or(false)
    }

    // Determine which model to use (priority: --model > --small > config.model)
    let model_ref = if let Some(model_flag) = get_string_flag(call, "model") {
        // --model flag takes highest priority
        model_flag
    } else if get_bool_flag(call, "small") {
        // --small flag uses small_model from config
        plugin_config.small_model.clone().ok_or_else(|| {
            LabeledError::new("No small model configured").with_label(
                "Set 'small_model' in plugin config to use --small flag",
                call.head,
            )
        })?
    } else {
        // Use default model from config
        plugin_config.model.clone()
    };

    // Resolve model to Config using PluginConfig
    let mut config = plugin_config
        .resolve_model(&model_ref)
        .map_err(|msg| LabeledError::new("Failed to resolve model").with_label(msg, call.head))?;

    // Step 3: Apply flag overrides for optional fields
    // These override any values from PluginConfig
    if let Some(api_key) = get_string_flag(call, "api-key") {
        config.api_key = Some(api_key);
    }
    if let Some(base_url) = get_string_flag(call, "base-url") {
        config.base_url = Some(base_url);
    }
    if let Some(temperature) = call
        .get_flag::<Value>("temperature")
        .ok()
        .flatten()
        .and_then(|v| v.as_float().ok())
    {
        config.temperature = Some(temperature);
    }
    if let Some(max_tokens) = call
        .get_flag::<Value>("max-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_tokens = Some(max_tokens);
    }
    if let Some(max_context) = call
        .get_flag::<Value>("max-context-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_context_tokens = Some(max_context);
    }
    if let Some(max_output) = call
        .get_flag::<Value>("max-output-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_output_tokens = Some(max_output);
    }
    if let Some(max_turns) = call
        .get_flag::<Value>("max-turns")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_tool_turns = Some(max_turns);
    }

    // Step 4: Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok(config)
}

/// OLD resolution flow for backward compatibility
fn resolve_with_old_config(
    plugin_config_opt: Option<Value>,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Step 1: Extract flag config first
    let flag_config = extract_flag_config(call);

    // Step 2: Determine provider/model for env lookup
    // Use plugin config if available, then flags, then default
    let (provider_hint, model_hint) = if let Some(ref plugin_value) = plugin_config_opt {
        // Try to extract provider/model from plugin config for env lookup
        let plugin_parsed = Config::from_plugin_config(plugin_value)?;
        (plugin_parsed.provider.clone(), plugin_parsed.model.clone())
    } else if !flag_config.provider.is_empty() && !flag_config.model.is_empty() {
        (flag_config.provider.clone(), flag_config.model.clone())
    } else {
        ("openai".to_string(), "gpt-4".to_string())
    };

    // Step 3: Start with defaults and merge environment config
    let env_config = Config::from_env(&provider_hint, &model_hint);
    let mut config = Config::default().merge(env_config);

    // Step 4: Merge plugin config if present
    if let Some(plugin_value) = plugin_config_opt {
        let plugin_config = Config::from_plugin_config(&plugin_value)?;
        config = config.merge(plugin_config);
    }

    // Step 5: Merge flag config (highest precedence) - only if values are non-empty
    // For required fields, only override if non-empty
    if !flag_config.provider.is_empty() {
        config.provider = flag_config.provider;
    }
    if !flag_config.model.is_empty() {
        config.model = flag_config.model;
    }
    // For optional fields, use standard merge
    config.api_key = flag_config.api_key.or(config.api_key);
    config.base_url = flag_config.base_url.or(config.base_url);
    config.temperature = flag_config.temperature.or(config.temperature);
    config.max_tokens = flag_config.max_tokens.or(config.max_tokens);
    config.max_context_tokens = flag_config.max_context_tokens.or(config.max_context_tokens);
    config.max_output_tokens = flag_config.max_output_tokens.or(config.max_output_tokens);
    config.max_tool_turns = flag_config.max_tool_turns.or(config.max_tool_turns);

    // Step 6: Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok(config)
}

#[cfg(test)]
#[path = "agent_test.rs"]
mod agent_test;

#[cfg(test)]
#[path = "prompt_test.rs"]
mod prompt_test;
