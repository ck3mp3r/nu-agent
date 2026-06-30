/// Maximum byte length of a tool result returned to the LLM before truncation.
/// No provider documents a per-result limit; this guards against total request body
/// size causing gateway rejections. Reference: opencode uses 50 KB, goose ~195 KB.
/// Kept as a reference constant and for use in tests; the runtime default is 20_000.
pub const MAX_TOOL_OUTPUT_BYTES: usize = 50_000;

/// If `output` exceeds `max_bytes`, writes the full content to a temp file and
/// returns the first `max_bytes` bytes (at a valid UTF-8 char boundary) plus a
/// message telling the LLM the file path and how to read more using the `read`
/// tool with offset and limit parameters.
/// Otherwise returns `output` unchanged.
///
/// Special case: if `max_bytes == 0`, truncation is disabled and `output` is
/// returned unchanged.
pub fn truncate_tool_output(output: String, max_bytes: usize) -> String {
    if max_bytes == 0 {
        return output;
    }

    let original_len = output.len();
    if original_len <= max_bytes {
        return output;
    }

    // Find the largest valid UTF-8 char boundary at or before max_bytes.
    let boundary = output.floor_char_boundary(max_bytes);
    let truncated_prefix = &output[..boundary];

    // Write full content to a uniquely-named temp file.
    // Combine a millisecond timestamp with a random 32-bit value to avoid
    // collisions when multiple tools complete within the same millisecond.
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let random_suffix: u32 = rand::random();
    let file_name = format!("nu-agent-tool-output-{timestamp_ms}-{random_suffix:08x}.txt");
    let file_path = std::env::temp_dir().join(&file_name);

    let suffix = match std::fs::write(&file_path, &output) {
        Ok(()) => {
            let path_str = file_path.display();
            format!(
                "\n[output truncated: {original_len} bytes total, showing first {max_bytes} bytes. \
Full output saved to: {path_str}. \
Use the `read` tool with offset and limit parameters to read more of the file.]"
            )
        }
        Err(_) => {
            format!(
                "\n[output truncated: {original_len} bytes total, showing first {max_bytes} bytes. \
Full output could not be saved.]"
            )
        }
    };

    format!("{truncated_prefix}{suffix}")
}

#[cfg(test)]
#[path = "limits_test.rs"]
mod limits_test;
