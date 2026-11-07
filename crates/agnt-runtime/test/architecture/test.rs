//! Architecture ban: event-payload identity fields are typed.
//!
//! Design note `6ad10855` §4.4 keeps only structure tests, and typed identity
//! is one of them: an identity field is a newtype, never a bare `String`. This
//! test reads the event vocabulary's source and fails when a `SystemEvent`
//! variant declares an identity field as a `String`.

use std::path::{Path, PathBuf};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const EVENTS_FILE: &str = "src/protocol/events.rs";
const SYSTEM_EVENT_HEADER: &str = "pub enum SystemEvent {";
const BARE_STRING_TYPES: [&str; 2] = ["String", "Option<String>"];

#[test]
fn test_architecture_typed_identity_event_payloads_are_typed() -> Result<()> {
    // -- Setup & Fixtures
    let source = std::fs::read_to_string(crate_path(EVENTS_FILE))?;
    let body = braced_block(&source, SYSTEM_EVENT_HEADER)?;

    // -- Exec
    let violations = bare_string_identity_fields(body);

    // -- Check
    assert!(
        violations.is_empty(),
        "`SystemEvent` payloads declare bare String identity fields: {violations:?}"
    );
    Ok(())
}

#[test]
fn test_architecture_typed_identity_check_flags_a_bare_string_id() -> Result<()> {
    // -- Setup & Fixtures: inline and block field shapes, plus the two shapes
    // the check must leave alone (a newtype identity, a non-identity String).
    let body = concat!(
        "    ToolStarted { call_id: String },\n",
        "    CompactionFailed {\n",
        "        source: String,\n",
        "        request_id: Option<String>,\n",
        "    },\n",
        "    SystemMessage { kind: SystemKind, tool_id: String },\n",
        "    PermissionRequested { request_id: RequestId, reason: String },\n",
        "    SessionStarted { session_identity: String },\n",
    );

    // -- Exec
    let violations = bare_string_identity_fields(body);

    // -- Check
    assert_eq!(
        violations,
        vec![
            "call_id".to_string(),
            "request_id".to_string(),
            "tool_id".to_string()
        ]
    );
    Ok(())
}

// region:    --- Test Support

fn crate_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The text between the braces that follow `header`.
fn braced_block<'a>(source: &'a str, header: &str) -> Result<&'a str> {
    let start = source
        .find(header)
        .ok_or_else(|| format!("`{header}` not found"))?
        + header.len();
    let mut depth = 1usize;
    for (offset, ch) in source[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&source[start..start + offset]);
                }
            }
            _ => {}
        }
    }
    Err(format!("`{header}` has no closing brace").into())
}

/// Field declarations typed as a bare `String` whose name marks an identity.
fn bare_string_identity_fields(body: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let fields = body.replace(['{', '}', ','], "\n");
    for field in fields.lines() {
        let Some((declaration, type_)) = field.split_once(':') else {
            continue;
        };
        if !BARE_STRING_TYPES.contains(&type_.trim()) {
            continue;
        }
        let name = declaration.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        if is_identity_name(name) {
            violations.push(name.to_string());
        }
    }
    violations
}

fn is_identity_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "id" || name.ends_with("_id")
}

// endregion: --- Test Support
