use super::*;

#[test]
fn interpret_name_rejects_non_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("name", noyalib::Value::Number(42.into()));

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_err());
    if let Err(FrontMatterError::InvalidField { key, expected, got }) = result {
        assert_eq!(key, "name");
        assert_eq!(expected, "string");
        assert_eq!(got, "number");
    } else {
        panic!("Expected InvalidField error");
    }
}

#[test]
fn interpret_defaults_when_no_front_matter() -> Result<()> {
    // No front matter at all: every optional field is None.
    let persona = interpret_front_matter(None, "body content".to_string())
        .map_err(|e| format!("interpret should succeed: {e:?}"))?;
    assert_eq!(persona.name, None);
    assert_eq!(persona.description, None);
    assert_eq!(persona.model, None);
    assert_eq!(persona.permissions, None);
    assert_eq!(persona.icon, None);
    assert_eq!(persona.body, "body content");

    // The config fields are None on the no-front-matter path too.
    let persona = interpret_front_matter(None, "body".to_string())
        .map_err(|e| format!("no-front-matter path should succeed: {e:?}"))?;
    assert_eq!(persona.temperature, None);
    assert_eq!(persona.max_tokens, None);
    assert_eq!(persona.max_tool_turns, None);
    assert_eq!(persona.max_tool_calls_per_subturn, None);
    assert_eq!(persona.max_tool_result_bytes, None);
    assert_eq!(persona.additional_params, None);

    // An empty mapping behaves like no front matter.
    let mapping = noyalib::Mapping::new();
    let persona = interpret_front_matter(Some(&mapping), "body".to_string())
        .map_err(|e| format!("interpret should succeed: {e:?}"))?;
    assert_eq!(persona.name, None);
    assert_eq!(persona.description, None);
    assert_eq!(persona.model, None);
    assert_eq!(persona.permissions, None);
    assert_eq!(persona.body, "body");
    Ok(())
}

#[test]
fn interpret_unknown_keys_ignored() -> Result<()> {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("name", noyalib::Value::String("test".to_string()));
    mapping.insert("unknown_key", noyalib::Value::String("ignored".to_string()));
    mapping.insert("another_unknown", noyalib::Value::Number(42.into()));

    let persona = interpret_front_matter(Some(&mapping), "body".to_string())
        .map_err(|e| format!("interpret should succeed: {e:?}"))?;
    assert_eq!(persona.name, Some("test".to_string()));
    assert_eq!(persona.description, None);
    Ok(())
}

#[test]
fn interpret_all_new_fields_parsed() -> Result<()> {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "temperature",
        noyalib::Value::Number(noyalib::Number::Float(0.7)),
    );
    mapping.insert(
        "max_tokens",
        noyalib::Value::Number(noyalib::Number::Integer(2048)),
    );
    mapping.insert(
        "max_tool_turns",
        noyalib::Value::Number(noyalib::Number::Integer(10)),
    );
    mapping.insert(
        "max_tool_calls_per_subturn",
        noyalib::Value::Number(noyalib::Number::Integer(5)),
    );
    mapping.insert(
        "max_tool_result_bytes",
        noyalib::Value::Number(noyalib::Number::Integer(10000)),
    );
    let mut params = noyalib::Mapping::new();
    params.insert("thinking", noyalib::Value::String("enabled".to_string()));
    mapping.insert("additional_params", noyalib::Value::Mapping(params));

    let persona = interpret_front_matter(Some(&mapping), "body".to_string())
        .map_err(|e| format!("should parse all new fields: {e:?}"))?;

    assert_eq!(persona.temperature, Some(0.7));
    assert_eq!(persona.max_tokens, Some(2048));
    assert_eq!(persona.max_tool_turns, Some(10));
    assert_eq!(persona.max_tool_calls_per_subturn, Some(5));
    assert_eq!(persona.max_tool_result_bytes, Some(10000));
    assert!(persona.additional_params.is_some());
    let additional_params = persona
        .additional_params
        .ok_or("additional_params should be present")?;
    assert_eq!(additional_params["thinking"], "enabled");
    Ok(())
}

#[test]
fn interpret_new_fields_wrong_type_errors() {
    let bad_cases: &[(&str, noyalib::Value)] = &[
        ("temperature", noyalib::Value::String("hot".to_string())),
        ("max_tokens", noyalib::Value::String("many".to_string())),
        ("max_tool_turns", noyalib::Value::String("lots".to_string())),
        (
            "max_tool_calls_per_subturn",
            noyalib::Value::String("several".to_string()),
        ),
        (
            "max_tool_result_bytes",
            noyalib::Value::String("big".to_string()),
        ),
        (
            "additional_params",
            noyalib::Value::String("not-a-map".to_string()),
        ),
    ];

    for (key, bad_value) in bad_cases {
        let mut mapping = noyalib::Mapping::new();
        mapping.insert(*key, bad_value.clone());
        let result = interpret_front_matter(Some(&mapping), "body".to_string());
        assert!(result.is_err(), "expected error for key={key}, got ok");
        if let Err(FrontMatterError::InvalidField { key: k, .. }) = result {
            assert_eq!(&k, key, "wrong key in error for {key}");
        } else {
            panic!("expected InvalidField error for key={key}, got different error type");
        }
    }
}

#[test]
fn interpret_integer_fields_reject_negatives() {
    let integer_fields = [
        "max_tokens",
        "max_tool_turns",
        "max_tool_calls_per_subturn",
        "max_tool_result_bytes",
    ];
    for field in &integer_fields {
        let mut mapping = noyalib::Mapping::new();
        mapping.insert(*field, noyalib::Value::Number(noyalib::Number::Integer(-1)));
        let result = interpret_front_matter(Some(&mapping), "body".to_string());
        assert!(
            result.is_err(),
            "expected error for negative {field}, got ok"
        );
    }
}

#[test]
fn parse_full_front_matter_with_config_fields() -> Result<()> {
    // End-to-end: parse a persona Markdown string through
    // PulldownCmarkFrontMatterParser + interpret_front_matter.
    // Tests the flat numeric fields; additional_params (nested YAML) is
    // covered by interpret_all_new_fields_parsed which constructs the
    // noyalib Mapping directly.
    let input = "---\n\
name: coder\n\
description: A focused coding agent\n\
model: openai/gpt-4o\n\
temperature: 0.2\n\
max_tokens: 8192\n\
max_tool_turns: 20\n\
max_tool_calls_per_subturn: 3\n\
max_tool_result_bytes: 50000\n\
---\n\
\n\
You are a focused coding agent.\n";

    let parser = PulldownCmarkFrontMatterParser;
    let raw = parser
        .parse(input)
        .map_err(|e| format!("parse should succeed: {e:?}"))?;
    let persona = interpret_front_matter(raw.front_matter.as_ref(), raw.body)
        .map_err(|e| format!("interpret should succeed: {e:?}"))?;

    assert_eq!(persona.name, Some("coder".to_string()));
    assert_eq!(
        persona.description,
        Some("A focused coding agent".to_string())
    );
    assert_eq!(persona.model, Some("openai/gpt-4o".to_string()));
    let temperature = persona.temperature.ok_or("temperature should be Some")?;
    assert!(
        (temperature - 0.2).abs() < 1e-9,
        "temperature should be approximately 0.2"
    );
    assert_eq!(persona.max_tokens, Some(8192));
    assert_eq!(persona.max_tool_turns, Some(20));
    assert_eq!(persona.max_tool_calls_per_subturn, Some(3));
    assert_eq!(persona.max_tool_result_bytes, Some(50000));
    assert_eq!(persona.additional_params, None);
    assert!(
        persona.body.contains("focused coding agent"),
        "body should contain 'focused coding agent'"
    );
    Ok(())
}

#[test]
fn interpret_all_fields_together() -> Result<()> {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("name", noyalib::Value::String("full-agent".to_string()));
    mapping.insert(
        "description",
        noyalib::Value::String("A complete agent".to_string()),
    );
    mapping.insert("model", noyalib::Value::String("claude-3".to_string()));
    mapping.insert("icon", noyalib::Value::String("🧠".to_string()));
    let mut perms = noyalib::Mapping::new();
    perms.insert("*", noyalib::Value::String("allow".to_string()));
    mapping.insert("permissions", noyalib::Value::Mapping(perms.clone()));

    let persona = interpret_front_matter(Some(&mapping), "body content".to_string())
        .map_err(|e| format!("interpret should succeed: {e:?}"))?;
    assert_eq!(persona.name, Some("full-agent".to_string()));
    assert_eq!(persona.description, Some("A complete agent".to_string()));
    assert_eq!(persona.model, Some("claude-3".to_string()));
    assert_eq!(persona.icon.as_deref(), Some("🧠"));
    let extracted_perms = persona.permissions.ok_or("permissions should be present")?;
    assert_eq!(extracted_perms, perms);
    assert_eq!(persona.body, "body content");
    Ok(())
}
