use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// AgentCapabilities
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentCapabilities {
    #[serde(default = "default_true")]
    pub streaming: bool,
    #[serde(rename = "pushNotifications", default)]
    pub push_notifications: bool,
    #[serde(default = "default_true")]
    pub stateful: bool,
    #[serde(rename = "extendedAgentCard", default)]
    pub extended_agent_card: bool,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            streaming: true,
            push_notifications: false,
            stateful: true,
            extended_agent_card: false,
        }
    }
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Skill
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<SkillInput>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<SkillOutput>>,
}

// ---------------------------------------------------------------------------
// SkillInput / SkillOutput
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillInput {
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillOutput {
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// AgentProvider
// ---------------------------------------------------------------------------

/// Provider information for an A2A agent (A2A spec §8.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    pub organization: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

// ---------------------------------------------------------------------------
// AgentInterface
// ---------------------------------------------------------------------------

/// An A2A protocol interface exposed by the agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterface {
    pub url: String,
    pub protocol_version: String,
    #[serde(rename = "protocolBinding")]
    pub protocol_binding: String,
}

// ---------------------------------------------------------------------------
// SecurityScheme — A2A / OpenAPI security scheme types
// ---------------------------------------------------------------------------

/// An API key-based security scheme (OpenAPI `apiKey` type).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySecurityScheme {
    /// The header or query parameter name.
    pub name: String,
    /// The location of the API key: "header" or "query".
    #[serde(rename = "in")]
    pub location: String,
}

/// An HTTP authentication security scheme (OpenAPI `http` type).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthSecurityScheme {
    /// The HTTP authorization scheme (e.g., "bearer", "basic").
    pub scheme: String,
}

/// Available OAuth 2.0 flows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlows {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implicit: Option<OAuthFlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<OAuthFlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_credentials: Option<OAuthFlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_code: Option<OAuthFlow>,
}

/// A single OAuth 2.0 flow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
}

/// An OAuth 2.0 security scheme.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2SecurityScheme {
    pub flows: OAuthFlows,
}

/// An OpenID Connect security scheme.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenIdConnectSecurityScheme {
    pub open_id_connect_url: String,
}

/// A mutual TLS security scheme (no additional configuration).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutualTlsSecurityScheme {}

/// Discriminated union of A2A / OpenAPI security scheme types.
///
/// Serialised with a `type` discriminator per the OpenAPI specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum SecurityScheme {
    #[serde(rename = "apiKey")]
    ApiKey(ApiKeySecurityScheme),
    #[serde(rename = "http")]
    HttpAuth(HttpAuthSecurityScheme),
    #[serde(rename = "oauth2")]
    OAuth2(Box<OAuth2SecurityScheme>),
    #[serde(rename = "openIdConnect")]
    OpenIdConnect(OpenIdConnectSecurityScheme),
    #[serde(rename = "mutualTls")]
    MutualTls(MutualTlsSecurityScheme),
}

// ---------------------------------------------------------------------------
// AgentCard
// ---------------------------------------------------------------------------

/// The well-known Agent Card describing an A2A agent (A2A spec §4.4.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    #[serde(default)]
    pub supported_interfaces: Vec<AgentInterface>,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub capabilities: AgentCapabilities,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub security_schemes: HashMap<String, SecurityScheme>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
    #[serde(rename = "defaultInputModes", default = "default_text_plain_vec")]
    pub default_input_modes: Vec<String>,
    #[serde(rename = "defaultOutputModes", default = "default_text_plain_vec")]
    pub default_output_modes: Vec<String>,
}

fn default_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn default_text_plain_vec() -> Vec<String> {
    vec!["text/plain".to_string()]
}

impl Default for AgentCard {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
            url: String::new(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            supported_interfaces: vec![],
            version: default_version(),
            capabilities: AgentCapabilities::default(),
            skills: vec![],
            security_schemes: HashMap::new(),
            extensions: vec![],
            metadata: None,
            default_input_modes: default_text_plain_vec(),
            default_output_modes: default_text_plain_vec(),
        }
    }
}

/// Rebuild an AgentCard for an agent switch, preserving server-bound fields
/// (url, supported_interfaces, version, security_schemes, extensions, metadata,
/// default_input_modes, default_output_modes) and updating persona-derived fields
/// (name, description, skills).
pub fn rebuild_card_for_switch(
    old: &AgentCard,
    new_name: &str,
    new_description: Option<&str>,
    new_skills: Vec<Skill>,
) -> AgentCard {
    AgentCard {
        name: new_name.to_string(),
        description: new_description.map(|s| s.to_string()),
        url: old.url.clone(),
        provider: old.provider.clone(),
        icon_url: old.icon_url.clone(),
        documentation_url: old.documentation_url.clone(),
        supported_interfaces: old.supported_interfaces.clone(),
        version: old.version.clone(),
        capabilities: old.capabilities.clone(),
        skills: new_skills,
        security_schemes: old.security_schemes.clone(),
        extensions: old.extensions.clone(),
        metadata: old.metadata.clone(),
        default_input_modes: old.default_input_modes.clone(),
        default_output_modes: old.default_output_modes.clone(),
    }
}

/// Synthesize a single Skill from a persona's name and description.
/// Mirrors the inline logic at run_command.rs:261-267.
pub fn skill_from_persona(name: &str, description: Option<&str>) -> Skill {
    Skill {
        id: name.to_string(),
        name: name.to_string(),
        description: description.unwrap_or_default().to_string(),
        inputs: None,
        outputs: None,
    }
}
