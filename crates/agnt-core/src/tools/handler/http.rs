use std::path::Path;
use std::time::Duration;

use serde_json::Value as JsonValue;

use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

const DEFAULT_MAX_LENGTH: usize = 12000;
const DEFAULT_MODE: &str = "markdown";

#[derive(Debug, serde::Deserialize)]
pub struct HttpArgs {
    pub url: String,
    #[serde(default)]
    pub mode: Option<String>, // "markdown" | "raw" — default: "markdown"
    #[serde(default)]
    pub max_length: Option<usize>, // default: 12000
}

/// Convert an HTML body to Markdown using htmd, skipping boilerplate tags.
fn html_to_markdown(html: &str) -> String {
    htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "nav", "header", "footer"])
        .build()
        .convert(html)
        .unwrap_or_else(|_| html.to_string())
}

/// Process a fetched HTTP body: optionally convert HTML to Markdown, then truncate.
///
/// Returns `(content, truncated)`.
pub fn process_body(
    body: String,
    content_type: &str,
    mode: &str,
    max_length: usize,
) -> (String, bool) {
    let is_html = content_type.starts_with("text/html");
    let converted = if mode == DEFAULT_MODE && is_html {
        html_to_markdown(&body)
    } else {
        body
    };

    let char_count = converted.chars().count();
    if char_count > max_length {
        let truncated_str = converted.char_indices().nth(max_length).map_or_else(
            || converted.clone(),
            |(idx, _)| converted[..idx].to_string(),
        );
        (truncated_str, true)
    } else {
        (converted, false)
    }
}

/// Build a minimal HTTP client suitable for single-page fetches.
///
/// Connect timeout: 10 s. Read timeout: 30 s (resets on each received chunk).
fn build_fetch_client() -> Result<reqwest::Client, ToolHandlerError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| ToolHandlerError::runtime(format!("Failed to build HTTP client: {e}")))
}

/// Validate that `url` is non-empty and starts with `http://` or `https://`.
fn validate_url(url: &str) -> Result<(), ToolHandlerError> {
    if url.is_empty() {
        return Err(ToolHandlerError::validation(
            "Invalid http arguments: url must not be empty",
        ));
    }

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ToolHandlerError::validation(format!(
            "Invalid http arguments: url must start with http:// or https://, got '{url}'"
        )));
    }

    Ok(())
}

pub struct HttpTool;

impl BuiltinTool for HttpTool {
    const NAME: &'static str = "http";

    /// Fetch a URL via HTTP GET, optionally converts HTML to Markdown, and
    /// truncates the result to `max_length` characters.
    async fn execute(
        arguments: &JsonValue,
        _cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: HttpArgs = serde_json::from_value(arguments.clone())
            .map_err(|e| ToolHandlerError::validation(format!("Invalid http arguments: {e}")))?;

        validate_url(&args.url)?;

        let effective_mode = args.mode.as_deref().unwrap_or(DEFAULT_MODE).to_string();
        let max_length = args.max_length.unwrap_or(DEFAULT_MAX_LENGTH);

        let client = build_fetch_client()?;

        let response = client
            .get(&args.url)
            .send()
            .await
            .map_err(|e| ToolHandlerError::runtime(format!("HTTP request failed: {e}")))?;

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let final_url = response.url().to_string();

        let body = response
            .text()
            .await
            .map_err(|e| ToolHandlerError::runtime(format!("Failed to read response body: {e}")))?;

        let (content, truncated) = process_body(body, &content_type, &effective_mode, max_length);
        let length = content.len();

        Ok(serde_json::json!({
            "url": final_url,
            "status": status,
            "content_type": content_type,
            "mode": effective_mode,
            "content": content,
            "length": length,
            "truncated": truncated,
        }))
    }
}
