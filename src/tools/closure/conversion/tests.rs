use super::{EngineInterfaceLike, closure_to_tool_definition};
use nu_protocol::{BlockId, Span, Spanned, engine::Closure};
use serde_json::json;

// Mock EngineInterface for testing
struct MockEngine {
    source: String,
}

impl EngineInterfaceLike for MockEngine {
    fn get_span_contents(&self, _span: Span) -> Result<Vec<u8>, String> {
        Ok(self.source.as_bytes().to_vec())
    }
}

#[test]
fn converts_closure_with_no_parameters() {
    // RED: Test closure with no parameters
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 7), // "{|| 42}"
    };

    let engine = MockEngine {
        source: "{|| 42}".to_string(),
    };

    let tool_def = closure_to_tool_definition(
        "constant".to_string(),
        &closure,
        &engine,
        Some("Returns 42".to_string()),
    );

    assert_eq!(tool_def.name, "constant");
    assert_eq!(tool_def.description, "Returns 42");

    // Verify schema structure - should have empty properties
    let schema = tool_def.parameters;
    assert_eq!(schema["type"], "object");
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.is_object());
    assert_eq!(
        properties.as_object().unwrap().len(),
        0,
        "Should have no parameters"
    );

    // Should have empty required array
    let required = schema.get("required").expect("Should have required field");
    assert_eq!(required.as_array().unwrap().len(), 0);
}

#[test]
fn converts_closure_with_one_parameter() {
    // RED: Test closure with one parameter
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 12), // "{|x| $x * 2}"
    };

    let engine = MockEngine {
        source: "{|x| $x * 2}".to_string(),
    };

    let tool_def = closure_to_tool_definition(
        "double".to_string(),
        &closure,
        &engine,
        Some("Double a number".to_string()),
    );

    assert_eq!(tool_def.name, "double");

    // Verify schema has one parameter named "x"
    let schema = tool_def.parameters;
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.get("x").is_some(), "Should have parameter 'x'");

    // Verify required array contains "x"
    let required = schema.get("required").expect("Should have required field");
    assert!(required.as_array().unwrap().contains(&json!("x")));
}

#[test]
fn converts_closure_with_two_parameters() {
    // RED: Test closure with two parameters
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 16), // "{|x, y| $x + $y}"
    };

    let engine = MockEngine {
        source: "{|x, y| $x + $y}".to_string(),
    };

    let tool_def = closure_to_tool_definition(
        "add".to_string(),
        &closure,
        &engine,
        Some("Add two numbers".to_string()),
    );

    assert_eq!(tool_def.name, "add");

    // Verify schema has two parameters
    let schema = tool_def.parameters;
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.get("x").is_some(), "Should have parameter 'x'");
    assert!(properties.get("y").is_some(), "Should have parameter 'y'");

    // Verify both are required
    let required = schema.get("required").expect("Should have required field");
    let req_array = required.as_array().unwrap();
    assert!(req_array.contains(&json!("x")), "x should be required");
    assert!(req_array.contains(&json!("y")), "y should be required");
    assert_eq!(
        req_array.len(),
        2,
        "Should have exactly 2 required parameters"
    );
}

#[test]
fn converts_closure_with_optional_parameter() {
    // RED: Test closure with optional parameter (marked with ?)
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 20), // "{|x, y?| $x + ($y? | default 0)}"
    };

    let engine = MockEngine {
        source: "{|x, y?| $x + ($y | default 0)}".to_string(),
    };

    let tool_def = closure_to_tool_definition("add_optional".to_string(), &closure, &engine, None);

    // Verify schema has both parameters
    let schema = tool_def.parameters;
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.get("x").is_some());
    assert!(properties.get("y").is_some());

    // Verify only x is required (y is optional)
    let required = schema.get("required").expect("Should have required field");
    let req_array = required.as_array().unwrap();
    assert!(req_array.contains(&json!("x")), "x should be required");
    assert!(!req_array.contains(&json!("y")), "y should not be required");
    assert_eq!(req_array.len(), 1);
}

#[test]
fn uses_default_description_when_none_provided() {
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(1),
            captures: vec![],
        },
        span: Span::new(0, 10),
    };

    let engine = MockEngine {
        source: "{|x| $x}".to_string(),
    };

    let tool_def = closure_to_tool_definition("identity".to_string(), &closure, &engine, None);

    assert_eq!(tool_def.name, "identity");
    assert!(tool_def.description.starts_with("Nushell closure tool:"));
    assert!(tool_def.description.contains("identity"));
}
