use nu_protocol::Value;

use super::input::{extract_context_from_input, extract_prompt_from_input};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_extract_prompt_from_string() -> Result<()> {
    let input = Value::test_string("What is Rust?");
    let result = extract_prompt_from_input(&input);

    let value = result.map_err(|e| format!("{e:?}"))?;
    assert_eq!(value, "What is Rust?");
    Ok(())
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
fn test_extract_context_from_string_input_returns_none() -> Result<()> {
    // String input has no context field
    let input = Value::test_string("What is Rust?");
    let result = extract_context_from_input(&input);

    let value = result.map_err(|e| format!("{e:?}"))?;
    assert_eq!(value, None);
    Ok(())
}

#[test]
fn test_extract_context_from_record_without_context_field() -> Result<()> {
    // Record with only prompt field has no context
    let input = Value::test_record(
        vec![("prompt".to_string(), Value::test_string("test prompt"))]
            .into_iter()
            .collect(),
    );
    let result = extract_context_from_input(&input);

    let value = result.map_err(|e| format!("{e:?}"))?;
    assert_eq!(value, None);
    Ok(())
}

#[test]
fn test_extract_context_from_record_with_context_field() -> Result<()> {
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

    let value = result.map_err(|e| format!("{e:?}"))?;
    assert_eq!(value, Some("Additional context information".to_string()));
    Ok(())
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
fn test_extract_context_accepts_empty_context() -> Result<()> {
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

    let value = result.map_err(|e| format!("{e:?}"))?;
    // Empty string should be treated as Some("") for now
    // The merging logic will decide how to handle it
    assert_eq!(value, Some("".to_string()));
    Ok(())
}

// ============================================================================
// Record Input Tests - Test record input with prompt and optional fields
// ============================================================================

mod record_input_tests {
    use super::extract_prompt_from_input;
    use nu_protocol::Value;

    type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn extract_prompt_from_string_input() -> Result<()> {
        // Test existing functionality - string input
        let input = Value::test_string("test prompt");
        let result = extract_prompt_from_input(&input);

        let value = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(value, "test prompt");
        Ok(())
    }

    #[test]
    fn extract_prompt_from_record_input_with_prompt_field() -> Result<()> {
        // RED: Test record input with {prompt: "test"}
        // This will fail because extract_prompt_from_input currently only accepts String

        let input = Value::test_record(
            vec![("prompt".to_string(), Value::test_string("test prompt"))]
                .into_iter()
                .collect(),
        );

        let result = extract_prompt_from_input(&input);

        // Expected: should extract "test prompt" from record
        let value = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(value, "test prompt");
        Ok(())
    }

    #[test]
    fn extract_prompt_from_record_rejects_missing_prompt_field() {
        // RED: Test that record without prompt field fails

        let input = Value::test_record(
            vec![("context".to_string(), Value::test_string("some context"))]
                .into_iter()
                .collect(),
        );

        let result = extract_prompt_from_input(&input);

        // Should fail with clear error about missing prompt
        assert!(result.is_err(), "Should reject record without prompt field");

        let err = result.unwrap_err();
        assert!(
            err.msg.contains("prompt") || err.msg.contains("required"),
            "Error should mention missing prompt: {}",
            err.msg
        );
    }

    #[test]
    fn extract_prompt_from_record_rejects_empty_prompt() {
        // RED: Test that record with empty prompt fails

        let input = Value::test_record(
            vec![("prompt".to_string(), Value::test_string(""))]
                .into_iter()
                .collect(),
        );

        let result = extract_prompt_from_input(&input);

        // Should fail for empty prompt
        assert!(result.is_err(), "Should reject empty prompt");

        let err = result.unwrap_err();
        assert!(
            err.msg.contains("empty") || err.msg.contains("prompt"),
            "Error should mention empty prompt: {}",
            err.msg
        );
    }

    #[test]
    fn extract_prompt_from_record_with_optional_fields() -> Result<()> {
        // RED: Test that record with optional fields (context, model) still works
        // For now, we just need to extract the prompt, optional fields are ignored

        let input = Value::test_record(
            vec![
                ("prompt".to_string(), Value::test_string("test prompt")),
                ("context".to_string(), Value::test_string("some context")),
                (
                    "model".to_string(),
                    Value::test_string("openai/gpt-3.5-turbo"),
                ),
            ]
            .into_iter()
            .collect(),
        );

        let result = extract_prompt_from_input(&input);

        let value = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(value, "test prompt");
        Ok(())
    }

    #[test]
    fn extract_prompt_rejects_invalid_types() {
        // Test that non-string, non-record inputs fail

        let input = Value::test_int(123);
        let result = extract_prompt_from_input(&input);

        assert!(result.is_err(), "Should reject integer input");

        let input = Value::test_bool(true);
        let result = extract_prompt_from_input(&input);

        assert!(result.is_err(), "Should reject boolean input");
    }
}
