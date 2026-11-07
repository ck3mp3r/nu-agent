use super::{MAX_TOOL_OUTPUT_BYTES, truncate_tool_output};

#[test]
fn truncate_tool_output_passthrough_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, String, usize)> = vec![
        (
            "short output",
            "hello, world".to_string(),
            MAX_TOOL_OUTPUT_BYTES,
        ),
        (
            "short output just under the limit",
            "a".repeat(MAX_TOOL_OUTPUT_BYTES - 1),
            MAX_TOOL_OUTPUT_BYTES,
        ),
        ("zero limit disables truncation", "x".repeat(100_000), 0),
        ("custom limit above the input", "a".repeat(25_000), 50_000),
        (
            "default limit above the input",
            "a".repeat(40_000),
            MAX_TOOL_OUTPUT_BYTES,
        ),
    ];

    // -- Exec & Check
    for (name, input, max_bytes) in cases {
        let result = truncate_tool_output(input.clone(), max_bytes);
        assert_eq!(
            result, input,
            "case '{name}': output must pass through unchanged"
        );
        assert!(
            !result.contains("[output truncated:"),
            "case '{name}': output must not contain a truncation marker"
        );
    }
}

#[test]
fn truncate_tool_output_truncation_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, String, usize)> = vec![
        (
            "default limit",
            "x".repeat(MAX_TOOL_OUTPUT_BYTES + 1_000),
            MAX_TOOL_OUTPUT_BYTES,
        ),
        ("custom limit", "a".repeat(25_000), 20_000),
        (
            "default limit with 60KB",
            "a".repeat(60_000),
            MAX_TOOL_OUTPUT_BYTES,
        ),
    ];

    // -- Exec & Check
    for (name, input, max_bytes) in cases {
        let original_len = input.len();
        let result = truncate_tool_output(input, max_bytes);

        assert!(
            result.len() < original_len,
            "case '{name}': truncated result must be shorter than the input"
        );
        assert!(
            result.contains(&format!("[output truncated: {original_len} bytes total")),
            "case '{name}': marker must carry the original length, got: {}",
            &result[result.len().saturating_sub(200)..]
        );
        assert!(
            result.contains(&format!("showing first {max_bytes} bytes")),
            "case '{name}': marker must mention the bytes shown"
        );

        let has_file =
            result.contains("nu-agent-tool-output-") && result.contains("Use the `read` tool");
        let has_fallback = result.contains("Full output could not be saved.");
        assert!(
            has_file || has_fallback,
            "case '{name}': marker must reference a temp file or the fallback, got: {}",
            &result[result.len().saturating_sub(300)..]
        );
    }
}

#[test]
fn long_output_temp_file_contains_full_content() {
    let original = "z".repeat(MAX_TOOL_OUTPUT_BYTES + 200);
    let result = truncate_tool_output(original.clone(), MAX_TOOL_OUTPUT_BYTES);

    // Extract file path from the result
    // Pattern: "Full output saved to: <path>."
    let marker = "Full output saved to: ";
    if let Some(start) = result.find(marker) {
        let after = &result[start + marker.len()..];
        // Path ends at ". Use the"
        let end = after
            .find(". Use the")
            .expect("expected '. Use the' after path");
        let file_path = &after[..end];

        let file_content =
            std::fs::read_to_string(file_path).expect("temp file should exist and be readable");
        assert_eq!(
            file_content, original,
            "temp file must contain the full original content"
        );

        // Clean up
        std::fs::remove_file(file_path).ok();
    } else {
        // If temp file write failed (CI without /tmp?), just check the fallback message
        assert!(
            result.contains("Full output could not be saved."),
            "expected file path or fallback message"
        );
    }
}

#[test]
fn truncation_respects_utf8_char_boundary() {
    // Build a string where a multi-byte character spans the boundary.
    // '€' is 3 bytes (0xE2 0x82 0xAC). We place it so it straddles MAX_TOOL_OUTPUT_BYTES.
    //
    // Fill with ASCII up to (MAX_TOOL_OUTPUT_BYTES - 2), then add '€' (3 bytes).
    // The last byte of '€' is at index MAX_TOOL_OUTPUT_BYTES, so slicing at
    // MAX_TOOL_OUTPUT_BYTES would cut the character in half.
    let prefix_len = MAX_TOOL_OUTPUT_BYTES - 2;
    let mut input = "a".repeat(prefix_len);
    input.push('€'); // adds 3 bytes: indices prefix_len, prefix_len+1, prefix_len+2
    // Total so far = prefix_len + 3 = MAX_TOOL_OUTPUT_BYTES + 1 → triggers truncation
    // Pad to make it clearly over the limit
    input.push_str(&"b".repeat(100));

    let result = truncate_tool_output(input, MAX_TOOL_OUTPUT_BYTES);

    // The truncated portion must be valid UTF-8 (Rust strings always are,
    // but we specifically check the truncation point is at a char boundary)
    // by verifying the prefix does NOT end with a partial '€'
    let truncation_start = result
        .find("\n[output truncated:")
        .expect("must have marker");
    let prefix = &result[..truncation_start];

    // Valid UTF-8 check: std::str::from_utf8 on bytes is the canonical test
    assert!(
        std::str::from_utf8(prefix.as_bytes()).is_ok(),
        "truncated prefix must be valid UTF-8"
    );

    // The boundary must be <= MAX_TOOL_OUTPUT_BYTES bytes
    assert!(
        prefix.len() <= MAX_TOOL_OUTPUT_BYTES,
        "prefix ({} bytes) must be <= MAX_TOOL_OUTPUT_BYTES ({})",
        prefix.len(),
        MAX_TOOL_OUTPUT_BYTES
    );

    // It should NOT include the '€' because that would require going to prefix_len+3
    // which is MAX_TOOL_OUTPUT_BYTES+1 — past the limit.
    // The boundary must be at prefix_len (just before '€').
    assert_eq!(
        prefix.len(),
        prefix_len,
        "truncation must fall at the char boundary just before '€'"
    );
}
