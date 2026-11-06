use super::{
    ClosureParameter, EngineInterfaceLike, closure_to_tool_definition, resolve_closure_params,
};
use nu_protocol::{BlockId, Span, Spanned, engine::Closure};
use serde_json::json;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

struct MockEngine {
    source: String,
}

impl EngineInterfaceLike for MockEngine {
    fn get_span_contents(&self, _span: Span) -> core::result::Result<Vec<u8>, String> {
        Ok(self.source.as_bytes().to_vec())
    }
}

#[test]
fn closure_to_tool_definition_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        params: Vec<ClosureParameter>,
        description: Option<&'static str>,
        expected_description: Option<&'static str>,
        expected_properties: Vec<&'static str>,
        expected_required: Vec<&'static str>,
    }

    let cases = vec![
        Case {
            name: "no parameters",
            params: super::parse_closure_parameters("{|| 42}"),
            description: Some("Returns 42"),
            expected_description: Some("Returns 42"),
            expected_properties: vec![],
            expected_required: vec![],
        },
        Case {
            name: "one parameter",
            params: super::parse_closure_parameters("{|x| $x * 2}"),
            description: Some("Double a number"),
            expected_description: Some("Double a number"),
            expected_properties: vec!["x"],
            expected_required: vec!["x"],
        },
        Case {
            name: "two parameters",
            params: super::parse_closure_parameters("{|x, y| $x + $y}"),
            description: Some("Add two numbers"),
            expected_description: Some("Add two numbers"),
            expected_properties: vec!["x", "y"],
            expected_required: vec!["x", "y"],
        },
        Case {
            name: "optional parameter",
            params: super::parse_closure_parameters("{|x, y?| $x + ($y | default 0)}"),
            description: None,
            expected_description: None,
            expected_properties: vec!["x", "y"],
            expected_required: vec!["x"],
        },
        Case {
            name: "explicit params without an engine",
            params: vec![
                ClosureParameter {
                    name: "city".to_string(),
                    is_required: true,
                },
                ClosureParameter {
                    name: "unit".to_string(),
                    is_required: false,
                },
            ],
            description: None,
            expected_description: None,
            expected_properties: vec!["city", "unit"],
            expected_required: vec!["city"],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let tool_def = closure_to_tool_definition(
            "tool".to_string(),
            &case.params,
            case.description.map(String::from),
        )?;

        assert_eq!(tool_def.name, "tool", "case '{}': name", case.name);
        match case.expected_description {
            Some(expected) => assert_eq!(
                tool_def.description, expected,
                "case '{}': description",
                case.name
            ),
            None => {
                assert!(
                    tool_def.description.starts_with("Nushell closure tool:"),
                    "case '{}': default description, got: {}",
                    case.name,
                    tool_def.description
                );
                assert!(
                    tool_def.description.contains("tool"),
                    "case '{}': default description must name the tool",
                    case.name
                );
            }
        }

        let schema = &tool_def.parameters;
        assert_eq!(
            schema["type"], "object",
            "case '{}': schema type",
            case.name
        );
        let properties = schema.get("properties").ok_or("Should have properties")?;
        assert!(properties.is_object(), "case '{}': properties", case.name);
        assert_eq!(
            properties.as_object().map(|o| o.len()),
            Some(case.expected_properties.len()),
            "case '{}': property count",
            case.name
        );
        for property in &case.expected_properties {
            assert!(
                properties.get(property).is_some(),
                "case '{}': property {property}",
                case.name
            );
            assert_eq!(
                properties[*property]["type"], "string",
                "case '{}': property {property} type",
                case.name
            );
        }

        let required = schema.get("required").ok_or("Should have required field")?;
        let required = required.as_array().ok_or("required must be an array")?;
        assert_eq!(
            required.len(),
            case.expected_required.len(),
            "case '{}': required count",
            case.name
        );
        for property in &case.expected_required {
            assert!(
                required.contains(&json!(property)),
                "case '{}': required must contain {property}",
                case.name
            );
        }
    }
    Ok(())
}

#[test]
fn uses_default_description_when_none_provided() -> Result<()> {
    let params = super::parse_closure_parameters("{|x| $x}");

    let tool_def = closure_to_tool_definition("identity".to_string(), &params, None)?;

    assert_eq!(tool_def.name, "identity");
    assert!(tool_def.description.starts_with("Nushell closure tool:"));
    assert!(tool_def.description.contains("identity"));
    Ok(())
}

#[test]
fn parse_closure_parameters_table() {
    // -- Setup & Fixtures
    type Case = (&'static str, &'static str, Vec<(&'static str, bool)>);

    let cases: Vec<Case> = vec![
        ("no parameters", "{|| 42}", vec![]),
        ("one parameter", "{|x| $x * 2}", vec![("x", true)]),
        (
            "two parameters",
            "{|x, y| $x + $y}",
            vec![("x", true), ("y", true)],
        ),
        (
            "optional parameter",
            "{|x, y?| $x + $y}",
            vec![("x", true), ("y", false)],
        ),
        (
            "whitespace",
            "{| x , y | $x + $y}",
            vec![("x", true), ("y", true)],
        ),
        (
            "spaced opening",
            "{ |x, y| $x + $y }",
            vec![("x", true), ("y", true)],
        ),
        (
            "typed param",
            "{|city_name: string| $city_name}",
            vec![("city_name", true)],
        ),
        (
            "optional typed param",
            "{|city_name?: string| $city_name}",
            vec![("city_name", false)],
        ),
        (
            "typed spaced",
            "{ |city_name: string| $city_name }",
            vec![("city_name", true)],
        ),
    ];

    // -- Exec & Check
    for (name, source, expected) in cases {
        let params = super::parse_closure_parameters(source);
        let expected: Vec<ClosureParameter> = expected
            .into_iter()
            .map(|(param_name, is_required)| ClosureParameter {
                name: param_name.to_string(),
                is_required,
            })
            .collect();
        assert_eq!(params, expected, "case '{name}'");
    }
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
fn closure_to_tool_definition_takes_params_not_engine() -> Result<()> {
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
    let def = closure_to_tool_definition("weather".to_string(), &params, None)?;
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
    Ok(())
}

#[test]
fn resolve_closure_params_returns_empty_on_engine_error() {
    struct FailingEngine;
    impl EngineInterfaceLike for FailingEngine {
        fn get_span_contents(&self, _span: Span) -> core::result::Result<Vec<u8>, String> {
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

#[test]
fn eval_closure_default_impl_returns_error() {
    let engine = MockEngine {
        source: String::new(),
    };
    let closure = create_test_closure();

    let result = engine.eval_closure(&closure, vec![], None);

    let err = result.err().unwrap_or_default();
    assert_eq!(
        err,
        "eval_closure not available for this engine implementation"
    );
}
