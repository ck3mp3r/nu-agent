use nu_protocol::Value;

use super::{extract_context_from_input, extract_prompt_from_input, merge_prompt_with_context};

#[test]
fn test_extract_prompt_from_string() {
    let input = Value::test_string("What is Rust?");
    let result = extract_prompt_from_input(&input);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "What is Rust?");
}

#[test]
fn test_extract_prompt_from_empty_string() {
    let input = Value::test_string("");
    let result = extract_prompt_from_input(&input);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.msg.contains("empty") || err.msg.contains("Empty"));
}

#[test]
fn test_extract_prompt_from_invalid_type_int() {
    let input = Value::test_int(42);
    let result = extract_prompt_from_input(&input);

    assert!(result.is_err());
    let err = result.unwrap_err();
    // Check that error indicates wrong type
    assert!(err.msg.contains("Invalid input type") || err.msg.contains("Expected a string"));
}

#[test]
fn test_extract_prompt_from_invalid_type_record() {
    // Record without 'prompt' field should fail
    let input = Value::test_record(
        vec![("key".to_string(), Value::test_string("value"))]
            .into_iter()
            .collect(),
    );
    let result = extract_prompt_from_input(&input);

    assert!(result.is_err());
    let err = result.unwrap_err();
    // Check that error indicates missing prompt field
    assert!(
        err.msg.contains("Missing required field") || err.msg.contains("prompt"),
        "Error message should mention missing prompt field, got: {}",
        err.msg
    );
}

#[test]
fn test_extract_prompt_from_nothing() {
    let input = Value::test_nothing();
    let result = extract_prompt_from_input(&input);

    assert!(result.is_err());
}

// ============================================================================
// Context Extraction Tests - Task 1.2
// ============================================================================

#[test]
fn test_extract_context_from_string_input_returns_none() {
    // String input has no context field
    let input = Value::test_string("What is Rust?");
    let result = extract_context_from_input(&input);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[test]
fn test_extract_context_from_record_without_context_field() {
    // Record with only prompt field has no context
    let input = Value::test_record(
        vec![("prompt".to_string(), Value::test_string("test prompt"))]
            .into_iter()
            .collect(),
    );
    let result = extract_context_from_input(&input);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[test]
fn test_extract_context_from_record_with_context_field() {
    // RED: Test that context field is extracted from record input
    let input = Value::test_record(
        vec![
            ("prompt".to_string(), Value::test_string("test prompt")),
            (
                "context".to_string(),
                Value::test_string("Additional context information"),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let result = extract_context_from_input(&input);

    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        Some("Additional context information".to_string())
    );
}

#[test]
fn test_extract_context_rejects_non_string_context() {
    // Context field must be a string
    let input = Value::test_record(
        vec![
            ("prompt".to_string(), Value::test_string("test prompt")),
            ("context".to_string(), Value::test_int(123)),
        ]
        .into_iter()
        .collect(),
    );
    let result = extract_context_from_input(&input);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.msg.contains("context") || err.msg.contains("Invalid"),
        "Error should mention context type issue, got: {}",
        err.msg
    );
}

#[test]
fn test_extract_context_accepts_empty_context() {
    // Empty context is valid (will be treated as None in merging)
    let input = Value::test_record(
        vec![
            ("prompt".to_string(), Value::test_string("test prompt")),
            ("context".to_string(), Value::test_string("")),
        ]
        .into_iter()
        .collect(),
    );
    let result = extract_context_from_input(&input);

    assert!(result.is_ok());
    // Empty string should be treated as Some("") for now
    // The merging logic will decide how to handle it
    assert_eq!(result.unwrap(), Some("".to_string()));
}

// ============================================================================
// Prompt Merging Tests - Task 1.2
// ============================================================================

#[test]
fn test_merge_prompt_without_context() {
    // When no context, return prompt as-is
    let prompt = "What is Rust?";
    let context: Option<&str> = None;

    let result = merge_prompt_with_context(prompt, context);

    assert_eq!(result, "What is Rust?");
}

#[test]
fn test_merge_prompt_with_context() {
    // RED: When context is provided, merge it with the prompt
    let prompt = "Explain the code";
    let context = Some("File: main.rs\nCode: fn main() { println!(\"Hello\"); }");

    let result = merge_prompt_with_context(prompt, context);

    // Context should be prepended to prompt with clear separation
    assert!(result.contains("File: main.rs"));
    assert!(result.contains("Explain the code"));
    assert!(result.len() > prompt.len());
}

#[test]
fn test_merge_prompt_with_empty_context() {
    // Empty context string should be treated as no context
    let prompt = "What is Rust?";
    let context = Some("");

    let result = merge_prompt_with_context(prompt, context);

    // Empty context should not affect the prompt
    assert_eq!(result, "What is Rust?");
}

#[test]
fn test_merge_prompt_with_whitespace_context() {
    // Whitespace-only context should be treated as no context
    let prompt = "What is Rust?";
    let context = Some("   \n  \t  ");

    let result = merge_prompt_with_context(prompt, context);

    // Whitespace-only context should not affect the prompt
    assert_eq!(result, "What is Rust?");
}

#[test]
fn test_merge_prompt_preserves_context_format() {
    // Verify that context formatting is preserved (newlines, etc.)
    let prompt = "Summarize";
    let context = Some("Line 1\nLine 2\nLine 3");

    let result = merge_prompt_with_context(prompt, context);

    // All context lines should be present
    assert!(result.contains("Line 1"));
    assert!(result.contains("Line 2"));
    assert!(result.contains("Line 3"));
    assert!(result.contains("Summarize"));
}
