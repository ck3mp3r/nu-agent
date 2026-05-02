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

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportType,
    pub url: Option<String>,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
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
            let args = get_optional_string_list(server_record, "args")?;
            let headers = get_optional_string_record(server_record, "headers")?;
            let env = get_optional_string_record(server_record, "env")?;

            servers.push(McpServerConfig {
                name: server_name.clone(),
                transport,
                url,
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
        }

        Ok(())
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
#[path = "config_tests.rs"]
mod config_tests;
