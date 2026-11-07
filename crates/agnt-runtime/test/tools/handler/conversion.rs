use super::{json_to_nu_value, nu_value_to_json};
use nu_protocol::{Span, Value};
use serde_json::json;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_tools_handler_conversion_json_to_nu_value_scalars() -> Result<()> {
    // -- Setup & Fixtures
    let span = Span::unknown();

    // -- Exec & Check
    assert_eq!(json_to_nu_value(&json!(null), span)?, Value::nothing(span));
    assert_eq!(
        json_to_nu_value(&json!(true), span)?,
        Value::bool(true, span)
    );
    assert_eq!(json_to_nu_value(&json!(42), span)?, Value::int(42, span));
    assert_eq!(json_to_nu_value(&json!(-7), span)?, Value::int(-7, span));
    assert_eq!(
        json_to_nu_value(&json!(1.5), span)?,
        Value::float(1.5, span)
    );
    assert_eq!(
        json_to_nu_value(&json!("hi"), span)?,
        Value::string("hi", span)
    );
    Ok(())
}

#[test]
fn test_tools_handler_conversion_json_to_nu_value_containers() -> Result<()> {
    // -- Setup & Fixtures
    let span = Span::unknown();
    let json = json!({"a": [1, "two", null], "b": {"c": true}});

    // -- Exec
    let value = json_to_nu_value(&json, span)?;

    // -- Check
    assert_eq!(nu_value_to_json(&value)?, json);
    Ok(())
}

#[test]
fn test_tools_handler_conversion_nu_value_to_json_scalars() -> Result<()> {
    // -- Setup & Fixtures
    let span = Span::unknown();

    // -- Exec & Check
    assert_eq!(nu_value_to_json(&Value::nothing(span))?, json!(null));
    assert_eq!(nu_value_to_json(&Value::bool(false, span))?, json!(false));
    assert_eq!(nu_value_to_json(&Value::int(-7, span))?, json!(-7));
    assert_eq!(nu_value_to_json(&Value::float(2.25, span))?, json!(2.25));
    assert_eq!(nu_value_to_json(&Value::string("s", span))?, json!("s"));
    Ok(())
}

#[test]
fn test_tools_handler_conversion_nu_value_to_json_containers() -> Result<()> {
    // -- Setup & Fixtures
    let span = Span::unknown();
    let value = Value::list(
        vec![
            Value::int(1, span),
            Value::record(
                nu_protocol::record! { "k" => Value::string("v", span) },
                span,
            ),
        ],
        span,
    );

    // -- Exec & Check
    assert_eq!(nu_value_to_json(&value)?, json!([1, {"k": "v"}]));
    Ok(())
}

#[test]
fn test_tools_handler_conversion_nu_value_to_json_unsupported_type_errors() -> Result<()> {
    // -- Setup & Fixtures
    let span = Span::unknown();

    // -- Exec
    let err = nu_value_to_json(&Value::duration(1, span))
        .err()
        .ok_or("duration must not convert to JSON")?;

    // -- Check
    assert_eq!(err.error, "Unsupported value type");
    Ok(())
}

#[test]
fn test_tools_handler_conversion_nu_value_to_json_non_finite_float_errors() -> Result<()> {
    // -- Setup & Fixtures
    let span = Span::unknown();

    // -- Exec
    let err = nu_value_to_json(&Value::float(f64::NAN, span))
        .err()
        .ok_or("NaN must not convert to JSON")?;

    // -- Check
    assert_eq!(err.error, "Invalid float value");
    Ok(())
}
