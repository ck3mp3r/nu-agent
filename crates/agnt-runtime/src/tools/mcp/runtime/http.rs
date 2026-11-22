//! The HTTP and SSE arm's transport config and client: the streamable-HTTP config
//! (headers, auth precedence, bearer resolution) and the shared client builder.

use http::{HeaderName, HeaderValue};

use crate::config::vault::Vault;
use crate::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};
use crate::utils::crypto::ensure_crypto_provider;

pub(crate) fn build_http_transport_config(
    server: &McpServerConfig,
    vault: Option<&Vault>,
) -> Result<rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig, String> {
    let (url, headers, allow_stateless) = match server.transport {
        McpTransportType::Sse => (
            server.url.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'sse' requires url",
                    server.name
                )
            })?,
            server.headers.clone(),
            true,
        ),
        McpTransportType::Http => (
            server.url.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'http' requires url",
                    server.name
                )
            })?,
            server.headers.clone(),
            false,
        ),
        McpTransportType::Stdio => {
            return Err("invalid transport type for HTTP config".to_string());
        }
    };

    let mut custom_headers = std::collections::HashMap::new();
    for (name, value) in headers {
        // When auth field is set, skip Authorization header — auth takes precedence
        if name.eq_ignore_ascii_case("Authorization") && !matches!(server.auth, McpAuthConfig::None)
        {
            log::warn!(
                "MCP server '{}' has both 'auth' and 'headers.Authorization' configured. \
                 The 'auth' field takes precedence.",
                server.name
            );
            continue;
        }
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("invalid MCP header name '{name}': {e}"))?;
        let header_value = HeaderValue::from_str(&value)
            .map_err(|e| format!("invalid MCP header value for '{name}': {e}"))?;
        custom_headers.insert(header_name, header_value);
    }

    let mut config =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url)
            .custom_headers(custom_headers);
    config.allow_stateless = allow_stateless;

    match &server.auth {
        McpAuthConfig::Bearer { token } => {
            // A `store:` token is resolved from the vault at connect time so a
            // rotated secret takes effect without editing config.toml. Without a
            // vault (or for a raw token) the configured value is used as-is.
            let resolved = match vault {
                Some(vault) => vault.resolve_bearer(token).map_err(|e| {
                    format!(
                        "MCP server '{}': failed to resolve bearer token: {e}",
                        server.name
                    )
                })?,
                None => token.clone(),
            };
            config = config.auth_header(resolved);
        }
        McpAuthConfig::None | McpAuthConfig::OAuth { .. } => {
            // No config-level auth header for None or OAuth
        }
    }

    Ok(config)
}

pub(crate) fn build_mcp_http_client(read_timeout_secs: u64) -> Result<reqwest::Client, String> {
    ensure_crypto_provider();
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(0) // match rmcp's default_http_client() — avoids Delayed ACK stall
        .pool_idle_timeout(Some(std::time::Duration::from_secs(55))); // evict before server closes
    if read_timeout_secs > 0 {
        builder = builder.read_timeout(std::time::Duration::from_secs(read_timeout_secs));
    }
    builder
        .build()
        .map_err(|e| format!("failed to build MCP HTTP client: {e}"))
}
