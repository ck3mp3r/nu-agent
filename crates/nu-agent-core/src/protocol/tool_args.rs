use serde_json::Value as JsonValue;

/// A tool call's transcript call line: the rendered summary text.
/// One string — no variants, no language tag. Preview content (diffs, code)
/// comes through `Previewable::preview()` → `Display`, not from this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallLine {
    pub summary: String,
}

impl CallLine {
    /// Flatten to the plain text a line-oriented renderer shows. The leading
    /// arrow is dropped so callers that add their own arrow do not double it
    /// (behavior carried over from the pre-Block-model enum).
    pub fn to_plain_text(&self) -> String {
        self.summary
            .strip_prefix("→ ")
            .unwrap_or(&self.summary)
            .to_string()
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
