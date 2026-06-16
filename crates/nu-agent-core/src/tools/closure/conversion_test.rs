use super::{
    ClosureParameter, EngineInterfaceLike, closure_to_tool_definition, resolve_closure_params,
};
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
    let params = super::parse_closure_parameters("{|| 42}");

    let tool_def = closure_to_tool_definition(
        "constant".to_string(),
        &params,
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
    let params = super::parse_closure_parameters("{|x| $x * 2}");

    let tool_def = closure_to_tool_definition(
        "double".to_string(),
        &params,
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
    let params = super::parse_closure_parameters("{|x, y| $x + $y}");

    let tool_def = closure_to_tool_definition(
        "add".to_string(),
        &params,
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
    let params = super::parse_closure_parameters("{|x, y?| $x + ($y | default 0)}");

    let tool_def = closure_to_tool_definition("add_optional".to_string(), &params, None);

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
    let params = super::parse_closure_parameters("{|x| $x}");

    let tool_def = closure_to_tool_definition("identity".to_string(), &params, None);

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

#[test]
fn parses_spaced_opening() {
    let params = super::parse_closure_parameters("{ |x, y| $x + $y }");
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[0],
        super::ClosureParameter {
            name: "x".to_string(),
            is_required: true
        }
    );
    assert_eq!(
        params[1],
        super::ClosureParameter {
            name: "y".to_string(),
            is_required: true
        }
    );
}

#[test]
fn parses_typed_param() {
    let params = super::parse_closure_parameters("{|city_name: string| $city_name}");
    assert_eq!(params.len(), 1);
    assert_eq!(
        params[0],
        super::ClosureParameter {
            name: "city_name".to_string(),
            is_required: true
        }
    );
}

#[test]
fn parses_optional_typed_param() {
    let params = super::parse_closure_parameters("{|city_name?: string| $city_name}");
    assert_eq!(params.len(), 1);
    assert_eq!(
        params[0],
        super::ClosureParameter {
            name: "city_name".to_string(),
            is_required: false
        }
    );
}

#[test]
fn parses_typed_spaced() {
    let params = super::parse_closure_parameters("{ |city_name: string| $city_name }");
    assert_eq!(params.len(), 1);
    assert_eq!(
        params[0],
        super::ClosureParameter {
            name: "city_name".to_string(),
            is_required: true
        }
    );
}

fn create_test_closure() -> Spanned<Closure> {
    Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::unknown(),
    }
}

#[test]
fn closure_to_tool_definition_takes_params_not_engine() {
    let params = vec![
        ClosureParameter {
            name: "city".to_string(),
            is_required: true,
        },
        ClosureParameter {
            name: "unit".to_string(),
            is_required: false,
        },
    ];
    let def = closure_to_tool_definition("weather".to_string(), &params, None);
    let schema: serde_json::Value = serde_json::from_str(&def.parameters.to_string()).unwrap();
    assert!(schema["properties"]["city"].is_object());
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("city"))
    );
    assert!(
        !schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("unit"))
    );
}

#[test]
fn resolve_closure_params_returns_empty_on_engine_error() {
    struct FailingEngine;
    impl EngineInterfaceLike for FailingEngine {
        fn get_span_contents(&self, _span: Span) -> Result<Vec<u8>, String> {
            Err("no active call context".to_string())
        }
    }
    let closure = create_test_closure();
    let params = resolve_closure_params(&closure, &FailingEngine);
    assert!(params.is_empty());
}

#[test]
fn resolve_closure_params_extracts_from_source() {
    let engine = MockEngine {
        source: "{|city_name: string| $city_name}".to_string(),
    };
    let closure = create_test_closure();
    let params = resolve_closure_params(&closure, &engine);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "city_name");
}
