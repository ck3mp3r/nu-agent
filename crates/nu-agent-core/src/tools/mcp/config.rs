use serde::Deserialize;

use crate::tools::mcp::MCP_TOOL_NAMESPACE_DELIMITER;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct McpConfig {
    pub mcp: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpTransportType {
    Stdio,
    Sse,
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum McpAuthConfig {
    #[default]
    None,
    Bearer {
        token: String,
    },
    OAuth {
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        client_secret: Option<String>,
        #[serde(default)]
        scope: Option<String>,
        #[serde(default)]
        redirect_uri: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportType,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub url: Option<String>,
    #[serde(default)]
    pub auth: McpAuthConfig,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

fn default_enabled() -> bool {
    true
}

impl McpConfig {
    pub fn from_plugin_config(
        value: &nu_protocol::Value,
    ) -> Result<Self, nu_protocol::LabeledError> {
        let record = value.as_record().map_err(|_| {
            nu_protocol::LabeledError::new("Invalid plugin configuration")
                .with_label("Expected a record for plugin configuration", value.span())
        })?;

        let Some(mcp_value) = record.get("mcp") else {
            return Ok(Self { mcp: Vec::new() });
        };

        let mcp_record = mcp_value.as_record().map_err(|_| {
            nu_protocol::LabeledError::new("Invalid MCP configuration")
                .with_label("'mcp' must be a record", mcp_value.span())
        })?;

        let mut servers = Vec::new();

        for (server_name, server_value) in mcp_record.iter() {
            let server_record = server_value.as_record().map_err(|_| {
                nu_protocol::LabeledError::new("Invalid MCP configuration").with_label(
                    format!("mcp.{server_name} must be a record"),
                    server_value.span(),
                )
            })?;

            let transport = get_required_string(server_record, "transport", server_value.span())?;
            let transport = parse_transport(transport, server_value.span())?;

            let url = get_optional_string(server_record, "url")?;
            let command = get_optional_string(server_record, "command")?;
            let cwd = get_optional_string(server_record, "cwd")?;
            let enabled = get_optional_bool(server_record, "enabled")?.unwrap_or(true);
            let args = get_optional_string_list(server_record, "args")?;
            let headers = get_optional_string_record(server_record, "headers")?;
            let auth = get_optional_auth(server_record, "auth")?;
            let env = get_optional_string_record(server_record, "env")?;

            servers.push(McpServerConfig {
                name: server_name.clone(),
                transport,
                enabled,
                url,
                auth,
                headers,
                command,
                cwd,
                args,
                env,
            });
        }

        let config = Self { mcp: servers };
        config.validate().map_err(|msg| {
            nu_protocol::LabeledError::new("Invalid MCP configuration")
                .with_label(msg, value.span())
        })?;

        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        for server in &self.mcp {
            if server.name.trim().is_empty() {
                return Err("MCP server name cannot be empty".to_string());
            }

            if server.name.contains(MCP_TOOL_NAMESPACE_DELIMITER) {
                return Err(format!(
                    "MCP server name '{}' contains reserved delimiter '{}'",
                    server.name, MCP_TOOL_NAMESPACE_DELIMITER
                ));
            }

            match server.transport {
                McpTransportType::Stdio => {
                    if server
                        .command
                        .as_deref()
                        .unwrap_or_default()
                        .trim()
                        .is_empty()
                    {
                        return Err(format!(
                            "MCP server '{}' with transport 'stdio' requires non-empty 'command'",
                            server.name
                        ));
                    }

                    if let Some(cwd) = server.cwd.as_deref()
                        && cwd.trim().is_empty()
                    {
                        return Err(format!(
                            "MCP server '{}' with transport 'stdio' requires non-empty 'cwd' when set",
                            server.name
                        ));
                    }
                }
                McpTransportType::Sse | McpTransportType::Http => {
                    if server.url.as_deref().unwrap_or_default().trim().is_empty() {
                        return Err(format!(
                            "MCP server '{}' with transport '{:?}' requires non-empty 'url'",
                            server.name, server.transport
                        ));
                    }
                }
            }

            // OAuth requires HTTP or SSE transport (not stdio)
            if matches!(server.auth, McpAuthConfig::OAuth { .. })
                && server.transport == McpTransportType::Stdio
            {
                return Err(format!(
                    "MCP server '{}' with auth type 'oauth' requires HTTP or SSE transport, not stdio",
                    server.name
                ));
            }

            // Bearer token must not be empty
            if let McpAuthConfig::Bearer { token } = &server.auth
                && token.trim().is_empty()
            {
                return Err(format!(
                    "MCP server '{}' has bearer auth with empty token",
                    server.name
                ));
            }

            // Warn if both auth.bearer and headers.Authorization are set
            if !matches!(server.auth, McpAuthConfig::None)
                && server.headers.contains_key("Authorization")
            {
                log::warn!(
                    "MCP server '{}' has both 'auth' and 'headers.Authorization' configured. \
                     The 'auth' field takes precedence.",
                    server.name
                );
            }
        }

        Ok(())
    }
}

fn get_optional_bool(
    record: &nu_protocol::Record,
    key: &str,
) -> Result<Option<bool>, nu_protocol::LabeledError> {
    match record.get(key) {
        Some(value) => value.as_bool().map(Some).map_err(|_| {
            nu_protocol::LabeledError::new("Invalid field type")
                .with_label(format!("'{key}' must be a bool"), value.span())
        }),
        None => Ok(None),
    }
}

fn get_required_string(
    record: &nu_protocol::Record,
    key: &str,
    span: nu_protocol::Span,
) -> Result<String, nu_protocol::LabeledError> {
    record
        .get(key)
        .ok_or_else(|| {
            nu_protocol::LabeledError::new("Missing required field")
                .with_label(format!("Missing '{key}' field"), span)
        })?
        .as_str()
        .map(|s| s.to_string())
        .map_err(|_| {
            nu_protocol::LabeledError::new("Invalid field type")
                .with_label(format!("'{key}' must be a string"), span)
        })
}

fn get_optional_string(
    record: &nu_protocol::Record,
    key: &str,
) -> Result<Option<String>, nu_protocol::LabeledError> {
    match record.get(key) {
        Some(value) => value.as_str().map(|s| Some(s.to_string())).map_err(|_| {
            nu_protocol::LabeledError::new("Invalid field type")
                .with_label(format!("'{key}' must be a string"), value.span())
        }),
        None => Ok(None),
    }
}

fn get_optional_string_list(
    record: &nu_protocol::Record,
    key: &str,
) -> Result<Vec<String>, nu_protocol::LabeledError> {
    let Some(value) = record.get(key) else {
        return Ok(Vec::new());
    };

    let list = value.as_list().map_err(|_| {
        nu_protocol::LabeledError::new("Invalid field type")
            .with_label(format!("'{key}' must be a list"), value.span())
    })?;

    list.iter()
        .map(|item| {
            item.as_str().map(|s| s.to_string()).map_err(|_| {
                nu_protocol::LabeledError::new("Invalid field type")
                    .with_label(format!("'{key}' entries must be strings"), item.span())
            })
        })
        .collect()
}

fn get_optional_string_record(
    record: &nu_protocol::Record,
    key: &str,
) -> Result<std::collections::HashMap<String, String>, nu_protocol::LabeledError> {
    let Some(value) = record.get(key) else {
        return Ok(std::collections::HashMap::new());
    };

    let map = value.as_record().map_err(|_| {
        nu_protocol::LabeledError::new("Invalid field type")
            .with_label(format!("'{key}' must be a record"), value.span())
    })?;

    let mut out = std::collections::HashMap::new();
    for (k, v) in map.iter() {
        let parsed = v.as_str().map_err(|_| {
            nu_protocol::LabeledError::new("Invalid field type")
                .with_label(format!("'{key}.{k}' must be a string"), v.span())
        })?;
        out.insert(k.clone(), parsed.to_string());
    }

    Ok(out)
}

fn get_optional_auth(
    record: &nu_protocol::Record,
    key: &str,
) -> Result<McpAuthConfig, nu_protocol::LabeledError> {
    let Some(value) = record.get(key) else {
        return Ok(McpAuthConfig::None);
    };

    let auth_record = value.as_record().map_err(|_| {
        nu_protocol::LabeledError::new("Invalid auth configuration").with_label(
            format!("'{key}' must be a record with a 'type' field"),
            value.span(),
        )
    })?;

    let auth_type = get_required_string(auth_record, "type", value.span())?;

    match auth_type.as_str() {
        "none" => Ok(McpAuthConfig::None),
        "bearer" => {
            let token = get_required_string(auth_record, "token", value.span())?;
            Ok(McpAuthConfig::Bearer { token })
        }
        "oauth" => {
            let client_id = get_optional_string(auth_record, "client-id")?;
            let client_secret = get_optional_string(auth_record, "client-secret")?;
            let scope = get_optional_string(auth_record, "scope")?;
            let redirect_uri = get_optional_string(auth_record, "redirect-uri")?;
            Ok(McpAuthConfig::OAuth {
                client_id,
                client_secret,
                scope,
                redirect_uri,
            })
        }
        other => Err(
            nu_protocol::LabeledError::new("Invalid auth type").with_label(
                format!("Unknown auth type '{other}'. Expected: none, bearer, oauth"),
                value.span(),
            ),
        ),
    }
}

fn parse_transport(
    transport: String,
    span: nu_protocol::Span,
) -> Result<McpTransportType, nu_protocol::LabeledError> {
    match transport.as_str() {
        "stdio" => Ok(McpTransportType::Stdio),
        "sse" => Ok(McpTransportType::Sse),
        "http" => Ok(McpTransportType::Http),
        _ => Err(nu_protocol::LabeledError::new("Invalid transport")
            .with_label(format!("unsupported transport '{transport}'"), span)),
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod config_test;
