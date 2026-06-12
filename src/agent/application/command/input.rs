use nu_protocol::{LabeledError, Value};

pub fn extract_prompt_from_input(input: &Value) -> Result<String, LabeledError> {
    // Try to extract as string first (original behavior)
    if let Ok(prompt_str) = input.as_str() {
        // Check for empty string
        if prompt_str.trim().is_empty() {
            return Err(LabeledError::new("Empty prompt")
                .with_label("Prompt cannot be empty", input.span()));
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
            return Err(LabeledError::new("Empty prompt")
                .with_label("Prompt cannot be empty", prompt_value.span()));
        }

        return Ok(prompt_str.to_string());
    }

    // Neither string nor record - error
    Err(LabeledError::new("Invalid input type").with_label(
        "Expected a string prompt or record with 'prompt' field",
        input.span(),
    ))
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
#[cfg(test)]
pub fn merge_prompt_with_context(prompt: &str, context: Option<&str>) -> String {
    crate::agent::protocol::prompt::merge_prompt_with_context(prompt, context)
}

pub(super) fn extract_prompt_and_context(
    input: &Value,
) -> Result<(String, Option<String>), LabeledError> {
    let prompt = extract_prompt_from_input(input)?;
    let context = extract_context_from_input(input)?;
    Ok((prompt, context))
}
