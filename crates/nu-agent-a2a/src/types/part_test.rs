use super::*;
use serde_json::json;

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

#[test]
fn role_roundtrip() {
    use serde_test::{Token, assert_tokens};

    assert_tokens(
        &Role::User,
        &[Token::UnitVariant {
            name: "Role",
            variant: "USER",
        }],
    );
    assert_tokens(
        &Role::Agent,
        &[Token::UnitVariant {
            name: "Role",
            variant: "AGENT",
        }],
    );
}

// ---------------------------------------------------------------------------
// Part
// ---------------------------------------------------------------------------

#[test]
fn part_text_roundtrip() {
    let part = Part::Text {
        text: "hello world".to_string(),
    };

    let json = serde_json::to_value(&part).expect("serialize");
    assert_eq!(json["text"], "hello world");
    assert!(
        json.get("type").is_none(),
        "untagged Part should not have a type key"
    );

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_text_exact_json() {
    let json_str = r#"{"type":"text","text":"hello"}"#;
    let part: Part = serde_json::from_str(json_str).expect("deserialize");
    assert_eq!(
        part,
        Part::Text {
            text: "hello".to_string()
        }
    );
}

#[test]
fn part_file_roundtrip_with_mime_type() {
    let part = Part::File {
        file: FileContent {
            url: "https://example.com/doc.pdf".to_string(),
            filename: "doc.pdf".to_string(),
            media_type: "application/pdf".to_string(),
        },
    };

    let json = serde_json::to_value(&part).expect("serialize");
    assert!(
        json.get("type").is_none(),
        "untagged Part should not have a type key"
    );
    assert_eq!(json["file"]["url"], "https://example.com/doc.pdf");
    assert_eq!(json["file"]["mediaType"], "application/pdf");

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_file_roundtrip_without_mime_type() {
    let part = Part::File {
        file: FileContent {
            url: "https://example.com/doc.pdf".to_string(),
            filename: "doc.pdf".to_string(),
            media_type: "application/pdf".to_string(),
        },
    };

    let json = serde_json::to_value(&part).expect("serialize");
    assert!(
        json.get("type").is_none(),
        "untagged Part should not have a type key"
    );
    assert_eq!(json["file"]["url"], "https://example.com/doc.pdf");
    assert_eq!(json["file"]["mediaType"], "application/pdf");

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_data_roundtrip() {
    let data = DataContent {
        media_type: "application/json".to_string(),
        schema: json!({"key": "value", "count": 42, "nested": {"a": [1, 2, 3]}}),
    };
    let part = Part::Data { data: data.clone() };

    let json = serde_json::to_value(&part).expect("serialize");
    assert!(
        json.get("type").is_none(),
        "untagged Part should not have a type key"
    );
    assert_eq!(json["data"]["mediaType"], "application/json");

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_unknown_type_fails() {
    let result: Result<Part, _> = serde_json::from_str(r#"{"type":"unknown","foo":"bar"}"#);
    assert!(
        result.is_err(),
        "deserializing an unknown Part variant should fail"
    );
}
