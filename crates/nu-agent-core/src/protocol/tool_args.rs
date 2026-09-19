use serde_json::Value as JsonValue;

/// How a tool call's arguments render on the transcript call line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallLineRender {
    /// A single-line summary, e.g. `→ {"path":"a.rs"}`.
    Inline { summary: String },
    /// A fenced code block with a language tag, e.g. a nu command.
    CodeBlock { language: String, code: String },
}

impl CallLineRender {
    /// Fallback render for tools without a tailored call line: the
    /// arrow-prefixed, truncated JSON arguments as an inline summary.
    pub fn generic_json_summary(arguments: &str) -> Self {
        Self::Inline {
            summary: format!("→ {}", summarize_tool_arguments(arguments)),
        }
    }

    /// Flatten to the plain text a line-oriented renderer shows. Inline
    /// summaries drop the leading arrow so callers that add their own arrow
    /// do not double it; code blocks yield the raw code.
    pub fn to_plain_text(&self) -> String {
        match self {
            Self::Inline { summary } => summary
                .strip_prefix("→ ")
                .unwrap_or(summary.as_str())
                .to_string(),
            Self::CodeBlock { code, .. } => code.clone(),
        }
    }
}

/// Parse `arguments` as JSON and return the named field as a string.
/// Returns `None` when the JSON is invalid, the field is missing, or the
/// value is not a string.
pub fn parse_json_string_field(arguments: &str, field: &str) -> Option<String> {
    let json = serde_json::from_str::<JsonValue>(arguments).ok()?;
    Some(json.get(field)?.as_str()?.to_string())
}

/// Parse `arguments` as JSON and return the named field as a `usize`.
/// Returns `None` when the JSON is invalid, the field is missing, or the
/// value is not an unsigned integer that fits in `usize`.
pub fn parse_json_usize_field(arguments: &str, field: &str) -> Option<usize> {
    let json = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let value = json.get(field)?.as_u64()?;
    usize::try_from(value).ok()
}

/// Parse `arguments` as JSON and return the length of the named array
/// field. Returns `None` when the JSON is invalid, the field is missing,
/// or the value is not an array.
pub fn parse_json_array_len(arguments: &str, field: &str) -> Option<usize> {
    let json = serde_json::from_str::<JsonValue>(arguments).ok()?;
    Some(json.get(field)?.as_array()?.len())
}

pub fn summarize_tool_arguments(arguments: &str) -> String {
    const MAX_LEN: usize = 120;
    let compact = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
    let spaced = compact.replace(", ", ",").replace(",", ", ");
    if spaced.chars().count() <= MAX_LEN {
        return spaced;
    }

    let mut truncated = spaced.chars().take(MAX_LEN).collect::<String>();
    truncated.push('…');
    truncated
}

/// Extract the raw nu command string from a nu tool-call arguments JSON
/// (`{"command": "..."}` shape). Returns `None` when the JSON is invalid,
/// the key is missing, or the value is not a non-empty string. CRLF line
/// endings are normalized to LF so the command renders one row per line.
pub fn nu_command_from_args(arguments: &str) -> Option<String> {
    let json = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let command = json.get("command")?.as_str()?;
    if command.is_empty() {
        return None;
    }
    Some(command.replace("\r\n", "\n").replace('\r', "\n"))
}
