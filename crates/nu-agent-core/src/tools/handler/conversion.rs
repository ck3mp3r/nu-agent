use nu_protocol::{Span, Value, shell_error::generic::GenericError};
use serde_json::Value as JsonValue;

/// Convert a serde_json::Value to nu_protocol::Value.
///
/// Recursively converts JSON values to their Nushell equivalents.
///
/// # Arguments
/// * `json` - The JSON value to convert
/// * `span` - The span for error reporting and value creation
///
/// # Returns
/// A Nushell Value, or ShellError if conversion fails
pub fn json_to_nu_value(json: &JsonValue, span: Span) -> Result<Value, Box<GenericError>> {
    match json {
        JsonValue::Null => Ok(Value::nothing(span)),
        JsonValue::Bool(b) => Ok(Value::bool(*b, span)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::int(i, span))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::float(f, span))
            } else {
                Err(Box::new(GenericError::new(
                    "Invalid JSON number",
                    "Could not convert number",
                    span,
                )))
            }
        }
        JsonValue::String(s) => Ok(Value::string(s.clone(), span)),
        JsonValue::Array(arr) => {
            let values: Result<Vec<Value>, Box<GenericError>> = arr
                .iter()
                .map(|item| json_to_nu_value(item, span))
                .collect();
            Ok(Value::list(values?, span))
        }
        JsonValue::Object(obj) => {
            let mut record = nu_protocol::record!();
            for (key, value) in obj {
                record.insert(key.clone(), json_to_nu_value(value, span)?);
            }
            Ok(Value::record(record, span))
        }
    }
}

/// Convert a nu_protocol::Value to serde_json::Value.
///
/// Recursively converts Nushell values to their JSON equivalents.
///
/// # Arguments
/// * `value` - The Nushell value to convert
///
/// # Returns
/// A JSON value, or ShellError if conversion fails
pub fn nu_value_to_json(value: &Value) -> Result<JsonValue, Box<GenericError>> {
    match value {
        Value::Nothing { .. } => Ok(JsonValue::Null),
        Value::Bool { val, .. } => Ok(JsonValue::Bool(*val)),
        Value::Int { val, .. } => Ok(JsonValue::Number((*val).into())),
        Value::Float { val, .. } => serde_json::Number::from_f64(*val)
            .map(JsonValue::Number)
            .ok_or_else(|| {
                Box::new(GenericError::new(
                    "Invalid float value",
                    "Cannot convert float to JSON",
                    value.span(),
                ))
            }),
        Value::String { val, .. } => Ok(JsonValue::String(val.clone())),
        Value::List { vals, .. } => {
            let json_values: Result<Vec<JsonValue>, Box<GenericError>> =
                vals.iter().map(nu_value_to_json).collect();
            Ok(JsonValue::Array(json_values?))
        }
        Value::Record { val, .. } => {
            let mut map = serde_json::Map::new();
            for (key, value) in val.iter() {
                map.insert(key.clone(), nu_value_to_json(value)?);
            }
            Ok(JsonValue::Object(map))
        }
        _ => Err(Box::new(GenericError::new(
            "Unsupported value type",
            format!("Cannot convert {:?} to JSON", value),
            value.span(),
        ))),
    }
}
