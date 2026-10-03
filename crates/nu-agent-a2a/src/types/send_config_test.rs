use serde_json::json;

use super::*;

// ---------------------------------------------------------------------------
// SendMessageConfiguration (spec §3.2.2)
// ---------------------------------------------------------------------------

#[test]
fn send_config_full_roundtrip() {
    let config = SendMessageConfiguration {
        return_immediately: Some(true),
        accepted_output_modes: Some(vec![
            "text/plain".to_string(),
            "application/json".to_string(),
        ]),
    };

    let json = serde_json::to_value(&config).expect("serialize");
    assert_eq!(json["returnImmediately"], true);
    assert_eq!(json["acceptedOutputModes"][0], "text/plain");

    let back: SendMessageConfiguration = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, config);
}

#[test]
fn send_config_minimal_omits_none_fields() {
    let config = SendMessageConfiguration::default();

    let json = serde_json::to_value(&config).expect("serialize");
    assert!(
        json.get("returnImmediately").is_none(),
        "returnImmediately should be absent when None, got: {json}"
    );
    assert!(
        json.get("acceptedOutputModes").is_none(),
        "acceptedOutputModes should be absent when None, got: {json}"
    );

    let back: SendMessageConfiguration = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, config);
}

#[test]
fn send_config_deserializes_camel_case_body() {
    let json = json!({
        "returnImmediately": false,
        "acceptedOutputModes": ["text/plain"]
    });

    let config: SendMessageConfiguration = serde_json::from_value(json).expect("deserialize");
    assert_eq!(config.return_immediately, Some(false));
    assert_eq!(
        config.accepted_output_modes,
        Some(vec!["text/plain".to_string()])
    );
}

#[test]
fn send_config_absent_return_immediately_means_blocking() {
    let config = SendMessageConfiguration::default();
    assert!(
        !config.is_return_immediately(),
        "absent returnImmediately must mean blocking"
    );

    let explicit_false = SendMessageConfiguration {
        return_immediately: Some(false),
        accepted_output_modes: None,
    };
    assert!(!explicit_false.is_return_immediately());

    let explicit_true = SendMessageConfiguration {
        return_immediately: Some(true),
        accepted_output_modes: None,
    };
    assert!(explicit_true.is_return_immediately());
}
