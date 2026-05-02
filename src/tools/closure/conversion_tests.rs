use super::{EngineInterfaceLike, closure_to_tool_definition};
use nu_protocol::{BlockId, Span, Spanned, engine::Closure};
use serde_json::json;

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
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 7),
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

    let schema = tool_def.parameters;
    assert_eq!(schema["type"], "object");
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.is_object());
    assert_eq!(properties.as_object().unwrap().len(), 0);

    let required = schema.get("required").expect("Should have required field");
    assert_eq!(required.as_array().unwrap().len(), 0);
}

#[test]
fn converts_closure_with_one_parameter() {
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 12),
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

    let schema = tool_def.parameters;
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.get("x").is_some());
    assert_eq!(properties["x"]["type"], "string");

    let required = schema.get("required").expect("Should have required field");
    assert!(required.as_array().unwrap().contains(&json!("x")));
}

#[test]
fn converts_closure_with_two_parameters() {
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 16),
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

    let schema = tool_def.parameters;
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.get("x").is_some());
    assert!(properties.get("y").is_some());
    assert_eq!(properties["x"]["type"], "string");
    assert_eq!(properties["y"]["type"], "string");

    let required = schema.get("required").expect("Should have required field");
    let req_array = required.as_array().unwrap();
    assert!(req_array.contains(&json!("x")));
    assert!(req_array.contains(&json!("y")));
    assert_eq!(req_array.len(), 2);
}

#[test]
fn converts_closure_with_optional_parameter() {
    let closure = Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::new(0, 20),
    };

    let engine = MockEngine {
        source: "{|x, y?| $x + ($y | default 0)}".to_string(),
    };

    let tool_def = closure_to_tool_definition("add_optional".to_string(), &closure, &engine, None);

    let schema = tool_def.parameters;
    let properties = schema.get("properties").expect("Should have properties");
    assert!(properties.get("x").is_some());
    assert!(properties.get("y").is_some());
    assert_eq!(properties["x"]["type"], "string");
    assert_eq!(properties["y"]["type"], "string");

    let required = schema.get("required").expect("Should have required field");
    let req_array = required.as_array().unwrap();
    assert!(req_array.contains(&json!("x")));
    assert!(!req_array.contains(&json!("y")));
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

#[test]
fn parses_no_parameters() {
    let params = super::parse_closure_parameters("{|| 42}");
    assert_eq!(params, vec![]);
}

#[test]
fn parses_one_parameter() {
    let params = super::parse_closure_parameters("{|x| $x * 2}");
    assert_eq!(
        params,
        vec![super::ClosureParameter {
            name: "x".to_string(),
            is_required: true
        }]
    );
}

#[test]
fn parses_two_parameters() {
    let params = super::parse_closure_parameters("{|x, y| $x + $y}");
    assert_eq!(
        params,
        vec![
            super::ClosureParameter {
                name: "x".to_string(),
                is_required: true
            },
            super::ClosureParameter {
                name: "y".to_string(),
                is_required: true
            },
        ]
    );
}

#[test]
fn parses_optional_parameter() {
    let params = super::parse_closure_parameters("{|x, y?| $x + $y}");
    assert_eq!(
        params,
        vec![
            super::ClosureParameter {
                name: "x".to_string(),
                is_required: true
            },
            super::ClosureParameter {
                name: "y".to_string(),
                is_required: false
            },
        ]
    );
}

#[test]
fn handles_whitespace() {
    let params = super::parse_closure_parameters("{| x , y | $x + $y}");
    assert_eq!(
        params,
        vec![
            super::ClosureParameter {
                name: "x".to_string(),
                is_required: true
            },
            super::ClosureParameter {
                name: "y".to_string(),
                is_required: true
            },
        ]
    );
}
