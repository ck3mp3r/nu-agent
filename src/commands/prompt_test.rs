use nu_protocol::Value;

use super::extract_prompt_from_input;

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
