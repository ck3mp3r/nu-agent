use super::http::{HttpArgs, dispatch_http_tool, process_body};

const DEFAULT_MAX_LENGTH: usize = 12000;

// --- parse_args tests ---

#[test]
fn parse_args_minimal() {
    let args: HttpArgs =
        serde_json::from_value(serde_json::json!({"url": "https://example.com"})).unwrap();

    assert_eq!(args.url, "https://example.com");
    assert!(args.mode.is_none());
    assert!(args.max_length.is_none());
}

#[test]
fn parse_args_raw_mode() {
    let args: HttpArgs =
        serde_json::from_value(serde_json::json!({"url": "https://example.com", "mode": "raw"}))
            .unwrap();

    assert_eq!(args.mode.as_deref(), Some("raw"));
}

#[test]
fn parse_args_custom_max_length() {
    let args: HttpArgs = serde_json::from_value(
        serde_json::json!({"url": "https://example.com", "max_length": 500}),
    )
    .unwrap();

    assert_eq!(args.max_length, Some(500));
}

// --- dispatch_http_tool error path tests ---

#[tokio::test]
async fn invalid_args_missing_url() {
    let result = dispatch_http_tool(&serde_json::json!({})).await;

    let err = result.expect_err("expected an error for missing url");
    assert_eq!(err.kind, super::ToolErrorKind::Validation);
    assert!(err.message.contains("Invalid"), "message: {}", err.message);
}

#[tokio::test]
async fn invalid_url_scheme() {
    let result = dispatch_http_tool(&serde_json::json!({"url": "ftp://example.com"})).await;

    let err = result.expect_err("expected an error for ftp scheme");
    assert_eq!(err.kind, super::ToolErrorKind::Validation);
    assert!(
        err.message.contains("http://") || err.message.contains("https://"),
        "message: {}",
        err.message
    );
}

// --- process_body tests ---

#[test]
fn truncation_applied() {
    let long_body = "x".repeat(DEFAULT_MAX_LENGTH + 500);
    let (content, truncated) = process_body(long_body, "text/plain", "raw", DEFAULT_MAX_LENGTH);

    assert!(truncated, "should be truncated");
    assert_eq!(content.len(), DEFAULT_MAX_LENGTH);
}

#[test]
fn no_truncation_when_short() {
    let short_body = "hello world".to_string();
    let (content, truncated) = process_body(short_body.clone(), "text/plain", "raw", DEFAULT_MAX_LENGTH);

    assert!(!truncated, "should not be truncated");
    assert_eq!(content, short_body);
}

#[test]
fn html_triggers_markdown_conversion() {
    let (content, _truncated) =
        process_body("<h1>Hello</h1>".to_string(), "text/html", "markdown", DEFAULT_MAX_LENGTH);

    assert!(
        content.contains("# Hello"),
        "expected markdown heading, got: {content}"
    );
    assert!(
        !content.contains("<h1>"),
        "expected no raw html tags, got: {content}"
    );
}

#[test]
fn raw_mode_skips_conversion() {
    let (content, _truncated) =
        process_body("<h1>Hello</h1>".to_string(), "text/html", "raw", DEFAULT_MAX_LENGTH);

    assert!(
        content.contains("<h1>"),
        "expected raw html preserved, got: {content}"
    );
}

#[test]
fn non_html_skips_conversion() {
    let (content, _truncated) = process_body(
        "<h1>Hello</h1>".to_string(),
        "application/json",
        "markdown",
        DEFAULT_MAX_LENGTH,
    );

    // No conversion: content_type is not text/html, so the html is left as-is
    assert!(
        content.contains("<h1>"),
        "expected no conversion for non-html content type, got: {content}"
    );
}

#[tokio::test]
async fn empty_url_is_invalid() {
    let result = dispatch_http_tool(&serde_json::json!({"url": ""})).await;
    let err = result.expect_err("expected an error for empty url");
    assert_eq!(err.kind, super::ToolErrorKind::Validation);
}

#[test]
fn truncation_respects_char_boundary_multibyte() {
    // 🦀 is 4 bytes — byte count >> char count
    let emoji_body = "🦀".repeat(DEFAULT_MAX_LENGTH + 10);
    let (content, truncated) = process_body(
        emoji_body,
        "text/plain",
        "raw",
        DEFAULT_MAX_LENGTH,
    );
    assert!(truncated, "should be truncated");
    assert_eq!(content.chars().count(), DEFAULT_MAX_LENGTH);
}
