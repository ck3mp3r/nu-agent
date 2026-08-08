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
    pub fn from_toml_config(config: &crate::config::PluginConfig) -> Result<Self, String> {
        match &config.mcp {
            Some(value) => Self::from_toml(value),
            None => Ok(Self { mcp: Vec::new() }),
        }
    }

    pub fn from_toml(value: &toml::Value) -> Result<Self, String> {
        let Some(mcp_table) = value.as_table() else {
            return Ok(Self { mcp: Vec::new() });
        };
        let mut servers = Vec::new();
        for (server_name, server_value) in mcp_table.iter() {
            let Some(server_table) = server_value.as_table() else {
                return Err(format!("mcp.{server_name} must be a table"));
            };
            let transport = server_table
                .get("transport")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| format!("mcp.{server_name} missing 'transport' field"))?;
            let transport = match transport {
                "stdio" => McpTransportType::Stdio,
                "sse" => McpTransportType::Sse,
                "http" => McpTransportType::Http,
                other => return Err(format!("unsupported transport '{other}'")),
            };
            let url = server_table
                .get("url")
                .and_then(toml::Value::as_str)
                .map(String::from);
            let command = server_table
                .get("command")
                .and_then(toml::Value::as_str)
                .map(String::from);
            let cwd = server_table
                .get("cwd")
                .and_then(toml::Value::as_str)
                .map(String::from);
            let enabled = server_table
                .get("enabled")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true);
            let args: Vec<String> = server_table
                .get("args")
                .and_then(toml::Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let headers = server_table
                .get("headers")
                .and_then(toml::Value::as_table)
                .map(|t| {
                    t.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let env = server_table
                .get("env")
                .and_then(toml::Value::as_table)
                .map(|t| {
                    t.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let auth = server_table
                .get("auth")
                .and_then(toml::Value::as_table)
                .map(|auth_table| {
                    let auth_type = auth_table
                        .get("type")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("none");
                    match auth_type {
                        "none" => McpAuthConfig::None,
                        "bearer" => McpAuthConfig::Bearer {
                            token: auth_table
                                .get("token")
                                .and_then(toml::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        },
                        "oauth" => McpAuthConfig::OAuth {
                            client_id: auth_table
                                .get("client-id")
                                .and_then(toml::Value::as_str)
                                .map(String::from),
                            client_secret: auth_table
                                .get("client-secret")
                                .and_then(toml::Value::as_str)
                                .map(String::from),
                            scope: auth_table
                                .get("scope")
                                .and_then(toml::Value::as_str)
                                .map(String::from),
                            redirect_uri: auth_table
                                .get("redirect-uri")
                                .and_then(toml::Value::as_str)
                                .map(String::from),
                        },
                        _ => McpAuthConfig::None,
                    }
                })
                .unwrap_or_default();
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
        config.validate()?;
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
#[cfg(test)]
#[path = "config_test.rs"]
mod config_test;
