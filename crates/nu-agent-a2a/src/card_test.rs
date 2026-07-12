use std::collections::HashMap;

use super::*;
use serde_json::json;

// ---------------------------------------------------------------------------
// AgentCard
// ---------------------------------------------------------------------------

#[test]
fn agent_card_full_roundtrip() {
    let card = AgentCard {
        name: "TestAgent".to_string(),
        description: Some("A test agent".to_string()),
        url: "https://example.com/agent".to_string(),
        version: "1.0.0".to_string(),
        provider: Some(AgentProvider {
            organization: "TestOrg".to_string(),
            url: Some("https://example.com".to_string()),
        }),
        icon_url: Some("https://example.com/icon.png".to_string()),
        documentation_url: Some("https://example.com/docs".to_string()),
        supported_interfaces: vec![AgentInterface {
            url: "https://example.com/agent".to_string(),
            protocol_version: "1.0".to_string(),
        }],
        capabilities: AgentCapabilities {
            streaming: false,
            push_notifications: true,
            stateful: false,
        },
        skills: vec![Skill {
            id: "skill-1".to_string(),
            name: "Greeter".to_string(),
            description: "Greets the user".to_string(),
            inputs: Some(vec![SkillInput {
                kind: "string".to_string(),
                description: "Name to greet".to_string(),
            }]),
            outputs: Some(vec![SkillOutput {
                kind: "string".to_string(),
                description: "Greeting message".to_string(),
            }]),
        }],
        security_schemes: HashMap::from([(
            "apiKey".to_string(),
            SecurityScheme::ApiKey(ApiKeySecurityScheme {
                name: "X-API-Key".to_string(),
                location: "header".to_string(),
            }),
        )]),
        extensions: vec!["ext1".to_string()],
        metadata: Some(HashMap::from([
            ("color".to_string(), json!("blue")),
            ("version".to_string(), json!("beta")),
        ])),
    };

    let json = serde_json::to_value(&card).expect("serialize");
    assert_eq!(json["name"], "TestAgent");
    assert_eq!(json["description"], "A test agent");
    assert_eq!(json["url"], "https://example.com/agent");
    assert_eq!(json["version"], "1.0.0");
    assert_eq!(json["provider"]["organization"], "TestOrg");
    assert_eq!(json["iconUrl"], "https://example.com/icon.png");
    assert_eq!(json["documentationUrl"], "https://example.com/docs");
    assert_eq!(json["supportedInterfaces"][0]["protocolVersion"], "1.0");
    assert_eq!(json["capabilities"]["streaming"], false);
    assert_eq!(json["capabilities"]["pushNotifications"], true);
    assert_eq!(json["capabilities"]["stateful"], false);
    assert_eq!(json["skills"][0]["id"], "skill-1");
    assert!(json.get("securitySchemes").is_some());
    assert_eq!(json["extensions"][0], "ext1");
    assert!(json.get("metadata").is_some());

    let back: AgentCard = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, card);
}

#[test]
fn agent_card_minimal_roundtrip() {
    let card = AgentCard {
        name: "MinimalAgent".to_string(),
        description: None,
        url: "https://example.com/agent".to_string(),
        version: "0.1.0".to_string(),
        capabilities: AgentCapabilities::default(),
        skills: vec![],
        ..Default::default()
    };

    let json = serde_json::to_value(&card).expect("serialize");
    assert_eq!(json["name"], "MinimalAgent");
    assert!(json.get("description").is_none());
    assert_eq!(json["url"], "https://example.com/agent");
    assert_eq!(json["version"], "0.1.0");
    assert!(json.get("provider").is_none());
    assert!(json.get("iconUrl").is_none());
    assert!(json.get("documentationUrl").is_none());
    assert_eq!(
        json["supportedInterfaces"],
        json!([]),
        "empty supported_interfaces should be an empty array"
    );
    assert!(json.get("securitySchemes").is_none());
    assert!(json.get("extensions").is_none());
    assert!(json.get("metadata").is_none());

    let back: AgentCard = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, card);
}

#[test]
fn agent_card_empty_skills_is_array() {
    let card = AgentCard {
        name: "NoSkills".to_string(),
        description: None,
        url: "https://example.com".to_string(),
        version: "1.0".to_string(),
        capabilities: AgentCapabilities::default(),
        skills: vec![],
        ..Default::default()
    };

    let json = serde_json::to_value(&card).expect("serialize");
    assert_eq!(json["skills"], json!([]), "skills should be an empty array");
}

#[test]
fn agent_card_default_version() {
    let card: AgentCard = serde_json::from_str(
        r#"{
            "name": "Agent",
            "url": "https://example.com",
            "skills": [],
            "capabilities": {}
        }"#,
    )
    .expect("deserialize");
    assert_eq!(card.version, "0.1.0");
}

#[test]
fn agent_card_forward_compat_unknown_fields() {
    // Unknown fields MUST be silently accepted for forward compatibility.
    let json_str = r#"{
        "name": "ForwardCompat",
        "url": "https://example.com",
        "version": "2.0",
        "capabilities": {},
        "skills": [],
        "unknown_field": "should_not_cause_error",
        "extra_object": {"a": 1}
    }"#;

    let card: AgentCard = serde_json::from_str(json_str).expect("deserialize");
    assert_eq!(card.name, "ForwardCompat");
}

// ---------------------------------------------------------------------------
// Skill
// ---------------------------------------------------------------------------

#[test]
fn skill_with_inputs_and_outputs() {
    let skill = Skill {
        id: "s1".to_string(),
        name: "Echo".to_string(),
        description: "Echoes input".to_string(),
        inputs: Some(vec![SkillInput {
            kind: "string".to_string(),
            description: "Input to echo".to_string(),
        }]),
        outputs: Some(vec![SkillOutput {
            kind: "string".to_string(),
            description: "Echoed output".to_string(),
        }]),
    };

    let json = serde_json::to_value(&skill).expect("serialize");
    assert_eq!(json["id"], "s1");
    assert_eq!(json["inputs"][0]["type"], "string");
    assert_eq!(json["outputs"][0]["type"], "string");

    let back: Skill = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, skill);
}

#[test]
fn skill_without_inputs_outputs() {
    let skill = Skill {
        id: "s2".to_string(),
        name: "Simple".to_string(),
        description: "Simple skill".to_string(),
        inputs: None,
        outputs: None,
    };

    let json = serde_json::to_value(&skill).expect("serialize");
    assert!(json.get("inputs").is_none());
    assert!(json.get("outputs").is_none());

    let back: Skill = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, skill);
}

// ---------------------------------------------------------------------------
// SkillInput / SkillOutput
// ---------------------------------------------------------------------------

#[test]
fn skill_input_roundtrip() {
    let input = SkillInput {
        kind: "number".to_string(),
        description: "A numeric value".to_string(),
    };

    let json = serde_json::to_value(&input).expect("serialize");
    assert_eq!(json["type"], "number");
    assert_eq!(json["description"], "A numeric value");

    let back: SkillInput = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, input);
}

#[test]
fn skill_output_roundtrip() {
    let output = SkillOutput {
        kind: "boolean".to_string(),
        description: "Success flag".to_string(),
    };

    let json = serde_json::to_value(&output).expect("serialize");
    assert_eq!(json["type"], "boolean");
    assert_eq!(json["description"], "Success flag");

    let back: SkillOutput = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, output);
}

// ---------------------------------------------------------------------------
// SecurityScheme
// ---------------------------------------------------------------------------

#[test]
fn security_scheme_api_key_roundtrip() {
    let scheme = SecurityScheme::ApiKey(ApiKeySecurityScheme {
        name: "X-API-Key".to_string(),
        location: "header".to_string(),
    });
    let json = serde_json::to_value(&scheme).expect("serialize");
    assert_eq!(json["type"], "apiKey");
    assert_eq!(json["name"], "X-API-Key");
    assert_eq!(json["in"], "header");
    let back: SecurityScheme = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, scheme);
}

#[test]
fn security_scheme_http_bearer_roundtrip() {
    let scheme = SecurityScheme::HttpAuth(HttpAuthSecurityScheme {
        scheme: "bearer".to_string(),
    });
    let json = serde_json::to_value(&scheme).expect("serialize");
    assert_eq!(json["type"], "http");
    assert_eq!(json["scheme"], "bearer");
    let back: SecurityScheme = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, scheme);
}

#[test]
fn security_scheme_oauth2_roundtrip() {
    let scheme = SecurityScheme::OAuth2(Box::new(OAuth2SecurityScheme {
        flows: OAuthFlows {
            authorization_code: Some(OAuthFlow {
                authorization_url: Some("https://auth.example.com/auth".to_string()),
                token_url: Some("https://auth.example.com/token".to_string()),
                refresh_url: None,
                scopes: Some(HashMap::from([(
                    "read".to_string(),
                    "Read access".to_string(),
                )])),
            }),
            implicit: None,
            password: None,
            client_credentials: None,
        },
    }));
    let json = serde_json::to_value(&scheme).expect("serialize");
    assert_eq!(json["type"], "oauth2");
    assert_eq!(
        json["flows"]["authorizationCode"]["authorizationUrl"],
        "https://auth.example.com/auth"
    );
    assert_eq!(
        json["flows"]["authorizationCode"]["tokenUrl"],
        "https://auth.example.com/token"
    );
    let back: SecurityScheme = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, scheme);
}

#[test]
fn security_scheme_open_id_connect_roundtrip() {
    let scheme = SecurityScheme::OpenIdConnect(OpenIdConnectSecurityScheme {
        open_id_connect_url: "https://auth.example.com/.well-known/openid-configuration"
            .to_string(),
    });
    let json = serde_json::to_value(&scheme).expect("serialize");
    assert_eq!(json["type"], "openIdConnect");
    assert_eq!(
        json["openIdConnectUrl"],
        "https://auth.example.com/.well-known/openid-configuration"
    );
    let back: SecurityScheme = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, scheme);
}

#[test]
fn security_scheme_mutual_tls_roundtrip() {
    let scheme = SecurityScheme::MutualTls(MutualTlsSecurityScheme {});
    let json = serde_json::to_value(&scheme).expect("serialize");
    assert_eq!(json["type"], "mutualTls");
    let back: SecurityScheme = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, scheme);
}

// ---------------------------------------------------------------------------
// AgentProvider
// ---------------------------------------------------------------------------

#[test]
fn agent_provider_roundtrip() {
    let provider = AgentProvider {
        organization: "MyOrg".to_string(),
        url: Some("https://myorg.example.com".to_string()),
    };
    let json = serde_json::to_value(&provider).expect("serialize");
    assert_eq!(json["organization"], "MyOrg");
    assert_eq!(json["url"], "https://myorg.example.com");
    let back: AgentProvider = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, provider);
}

#[test]
fn agent_provider_without_url() {
    let provider = AgentProvider {
        organization: "MyOrg".to_string(),
        url: None,
    };
    let json = serde_json::to_value(&provider).expect("serialize");
    assert_eq!(json["organization"], "MyOrg");
    assert!(json.get("url").is_none());
}

// ---------------------------------------------------------------------------
// AgentInterface
// ---------------------------------------------------------------------------

#[test]
fn agent_interface_roundtrip() {
    let iface = AgentInterface {
        url: "http://127.0.0.1:8080".to_string(),
        protocol_version: "1.0".to_string(),
    };
    let json = serde_json::to_value(&iface).expect("serialize");
    assert_eq!(json["url"], "http://127.0.0.1:8080");
    assert_eq!(json["protocolVersion"], "1.0");
    let back: AgentInterface = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, iface);
}

// ---------------------------------------------------------------------------
// AgentCard — forward compatibility and defaults
// ---------------------------------------------------------------------------

#[test]
fn agent_card_security_schemes_default_empty() {
    let card = AgentCard {
        name: "test".to_string(),
        url: "https://example.com".to_string(),
        ..Default::default()
    };
    let json = serde_json::to_value(&card).expect("serialize");
    assert!(
        json.get("securitySchemes").is_none(),
        "empty securitySchemes should be omitted"
    );
}

#[test]
fn agent_card_supported_interfaces_default_empty() {
    let card = AgentCard {
        name: "test".to_string(),
        url: "https://example.com".to_string(),
        ..Default::default()
    };
    let json = serde_json::to_value(&card).expect("serialize");
    assert_eq!(
        json["supportedInterfaces"],
        json!([]),
        "empty supportedInterfaces should be an empty array"
    );
}

#[test]
fn agent_card_forward_compat_old_json() {
    // Old JSON without new fields should deserialize with defaults.
    let json_str = r#"{
        "name": "CompatAgent",
        "url": "https://example.com",
        "version": "1.0",
        "capabilities": {},
        "skills": []
    }"#;
    let card: AgentCard = serde_json::from_str(json_str).expect("deserialize");
    assert_eq!(card.name, "CompatAgent");
    assert!(card.provider.is_none());
    assert!(card.icon_url.is_none());
    assert!(card.documentation_url.is_none());
    assert!(card.supported_interfaces.is_empty());
    assert!(card.security_schemes.is_empty());
    assert!(card.extensions.is_empty());
}

// ---------------------------------------------------------------------------
// AgentCapabilities
// ---------------------------------------------------------------------------

#[test]
fn agent_capabilities_defaults() {
    let caps = AgentCapabilities::default();
    assert!(caps.streaming, "streaming should default to true");
    assert!(
        !caps.push_notifications,
        "push_notifications should default to false"
    );
    assert!(caps.stateful, "stateful should default to true");
}

#[test]
fn agent_capabilities_roundtrip() {
    let caps = AgentCapabilities {
        streaming: false,
        push_notifications: true,
        stateful: false,
    };

    let json = serde_json::to_value(&caps).expect("serialize");
    assert_eq!(json["streaming"], false);
    assert_eq!(json["pushNotifications"], true);
    assert_eq!(json["stateful"], false);

    let back: AgentCapabilities = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, caps);
}

#[test]
fn agent_capabilities_deserialize_missing_fields_use_defaults() {
    let caps: AgentCapabilities = serde_json::from_str(r#"{}"#).expect("deserialize");
    assert!(caps.streaming);
    assert!(!caps.push_notifications);
    assert!(caps.stateful);
}

#[test]
fn agent_capabilities_partial_deserialize() {
    let caps: AgentCapabilities =
        serde_json::from_str(r#"{"streaming": false}"#).expect("deserialize");
    assert!(!caps.streaming);
    assert!(!caps.push_notifications); // default
    assert!(caps.stateful); // default
}
