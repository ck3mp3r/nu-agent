use super::http::{HttpArgs, HttpTool, process_body};
use crate::bus::Bus;
use crate::protocol::tool_args::CallLine;
use crate::tools::handler::builtin_tool::BuiltinTool;

const DEFAULT_MAX_LENGTH: usize = 12000;

// --- parse_args tests ---

#[test]
fn parse_args_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value, Option<&str>, Option<usize>)> = vec![
        (
            "minimal",
            serde_json::json!({"url": "https://example.com"}),
            None,
            None,
        ),
        (
            "raw mode",
            serde_json::json!({"url": "https://example.com", "mode": "raw"}),
            Some("raw"),
            None,
        ),
        (
            "custom max length",
            serde_json::json!({"url": "https://example.com", "max_length": 500}),
            None,
            Some(500),
        ),
    ];

    // -- Exec & Check
    for (name, value, expected_mode, expected_max_length) in cases {
        let args: HttpArgs = serde_json::from_value(value).expect("valid http args");
        assert_eq!(args.url, "https://example.com", "case '{name}': url");
        assert_eq!(args.mode.as_deref(), expected_mode, "case '{name}': mode");
        assert_eq!(
            args.max_length, expected_max_length,
            "case '{name}': max_length"
        );
    }
}

// --- HttpTool::execute error path tests ---

#[tokio::test]
async fn http_execute_error_table() -> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value, &[&str])> = vec![
        ("missing url", serde_json::json!({}), &["Invalid"]),
        (
            "ftp scheme",
            serde_json::json!({"url": "ftp://example.com"}),
            &["http://", "https://"],
        ),
        ("empty url", serde_json::json!({"url": ""}), &[]),
    ];

    // -- Exec & Check
    for (name, args, expected_fragments) in cases {
        let result = HttpTool::execute(&args, std::path::Path::new("/tmp"), &Bus::default()).await;
        let err = result.err().ok_or("expected an error for this url")?;
        assert_eq!(
            err.kind,
            super::ToolErrorKind::Validation,
            "case '{name}': kind"
        );
        assert!(
            expected_fragments.is_empty()
                || expected_fragments
                    .iter()
                    .any(|fragment| err.message.contains(fragment)),
            "case '{name}': message must contain one of {expected_fragments:?}, got: {}",
            err.message
        );
    }
    Ok(())
}

// --- process_body tests ---

#[test]
fn process_body_table() {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        body: String,
        content_type: &'static str,
        mode: &'static str,
        max_length: usize,
        expected_truncated: bool,
        expected_equals_body: bool,
        expected_contains: Option<&'static str>,
        expected_absent: Option<&'static str>,
        expected_bytes: Option<usize>,
        expected_chars: Option<usize>,
    }

    let cases = vec![
        Case {
            name: "truncation applied",
            body: "x".repeat(DEFAULT_MAX_LENGTH + 500),
            content_type: "text/plain",
            mode: "raw",
            max_length: DEFAULT_MAX_LENGTH,
            expected_truncated: true,
            expected_equals_body: false,
            expected_contains: None,
            expected_absent: None,
            expected_bytes: Some(DEFAULT_MAX_LENGTH),
            expected_chars: None,
        },
        Case {
            name: "no truncation when short",
            body: "hello world".to_string(),
            content_type: "text/plain",
            mode: "raw",
            max_length: DEFAULT_MAX_LENGTH,
            expected_truncated: false,
            expected_equals_body: true,
            expected_contains: None,
            expected_absent: None,
            expected_bytes: None,
            expected_chars: None,
        },
        Case {
            name: "html triggers markdown conversion",
            body: "<h1>Hello</h1>".to_string(),
            content_type: "text/html",
            mode: "markdown",
            max_length: DEFAULT_MAX_LENGTH,
            expected_truncated: false,
            expected_equals_body: false,
            expected_contains: Some("# Hello"),
            expected_absent: Some("<h1>"),
            expected_bytes: None,
            expected_chars: None,
        },
        Case {
            name: "raw mode skips conversion",
            body: "<h1>Hello</h1>".to_string(),
            content_type: "text/html",
            mode: "raw",
            max_length: DEFAULT_MAX_LENGTH,
            expected_truncated: false,
            expected_equals_body: false,
            expected_contains: Some("<h1>"),
            expected_absent: None,
            expected_bytes: None,
            expected_chars: None,
        },
        Case {
            name: "non html skips conversion",
            body: "<h1>Hello</h1>".to_string(),
            content_type: "application/json",
            mode: "markdown",
            max_length: DEFAULT_MAX_LENGTH,
            expected_truncated: false,
            expected_equals_body: false,
            expected_contains: Some("<h1>"),
            expected_absent: None,
            expected_bytes: None,
            expected_chars: None,
        },
        Case {
            name: "truncation respects char boundary multibyte",
            body: "🦀".repeat(DEFAULT_MAX_LENGTH + 10),
            content_type: "text/plain",
            mode: "raw",
            max_length: DEFAULT_MAX_LENGTH,
            expected_truncated: true,
            expected_equals_body: false,
            expected_contains: None,
            expected_absent: None,
            expected_bytes: None,
            expected_chars: Some(DEFAULT_MAX_LENGTH),
        },
    ];

    // -- Exec & Check
    for case in cases {
        let (content, truncated) = process_body(
            case.body.clone(),
            case.content_type,
            case.mode,
            case.max_length,
        );
        assert_eq!(
            truncated, case.expected_truncated,
            "case '{}': truncated",
            case.name
        );
        if case.expected_equals_body {
            assert_eq!(content, case.body, "case '{}': content", case.name);
        }
        if let Some(fragment) = case.expected_contains {
            assert!(
                content.contains(fragment),
                "case '{}': content must contain {fragment}, got: {content}",
                case.name
            );
        }
        if let Some(fragment) = case.expected_absent {
            assert!(
                !content.contains(fragment),
                "case '{}': content must not contain {fragment}, got: {content}",
                case.name
            );
        }
        if let Some(bytes) = case.expected_bytes {
            assert_eq!(content.len(), bytes, "case '{}': byte length", case.name);
        }
        if let Some(chars) = case.expected_chars {
            assert_eq!(
                content.chars().count(),
                chars,
                "case '{}': char count",
                case.name
            );
        }
    }
}

// === call_line_render ===

#[test]
fn http_call_line_render_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, Option<&str>)> = vec![
        (
            "shows explicit method and url",
            r#"{"url":"https://example.com","method":"POST"}"#,
            Some("→ POST https://example.com"),
        ),
        (
            "defaults method to get",
            r#"{"url":"https://example.com"}"#,
            Some("→ GET https://example.com"),
        ),
        ("falls back to generic on invalid json", "not-json", None),
    ];

    // -- Exec & Check
    for (name, args, expected) in cases {
        let render = HttpTool::call_line_render(args);
        match expected {
            Some(summary) => assert_eq!(
                render,
                CallLine {
                    summary: summary.to_string(),
                },
                "case '{name}'"
            ),
            None => assert_eq!(render, CallLine::from_json_summary(args), "case '{name}'"),
        }
    }
}
