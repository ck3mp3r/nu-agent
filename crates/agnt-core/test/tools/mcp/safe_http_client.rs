use super::validate_url;

#[test]
fn validate_url_rejects_http_cloud_metadata() {
    let err = validate_url("http://169.254.169.254/").expect_err("should reject cloud metadata");
    // http to non-localhost is caught by the HTTPS check first
    assert!(
        err.contains("insecure URL scheme"),
        "unexpected error: {err}"
    );
}

#[test]
fn validate_url_rejects_http_internal_service() {
    let err =
        validate_url("http://internal-service/").expect_err("should reject http non-localhost");
    assert!(
        err.contains("insecure URL scheme"),
        "unexpected error: {err}"
    );
}

#[test]
fn validate_url_allows_https_mcp_example() {
    assert!(validate_url("https://mcp.example.com/").is_ok());
}

#[test]
fn validate_url_allows_http_localhost_with_port() {
    assert!(validate_url("http://localhost:8080/").is_ok());
}

#[test]
fn validate_url_allows_http_127_0_0_1_with_port() {
    assert!(validate_url("http://127.0.0.1:8080/").is_ok());
}

#[test]
fn validate_url_rejects_http_link_local() {
    let err = validate_url("http://169.254.1.1/").expect_err("should reject link-local");
    // http to non-localhost is caught by the HTTPS check first
    assert!(
        err.contains("insecure URL scheme"),
        "unexpected error: {err}"
    );
}

#[test]
fn validate_url_rejects_https_link_local() {
    let err =
        validate_url("https://169.254.1.1/").expect_err("should reject link-local even with https");
    assert!(err.contains("blocked host"), "unexpected error: {err}");
}

#[test]
fn validate_url_rejects_invalid_url() {
    let err = validate_url("not a url").expect_err("should reject invalid URL");
    assert!(err.contains("invalid URL"), "unexpected error: {err}");
}

#[test]
fn validate_url_rejects_https_cloud_metadata() {
    let err = validate_url("https://169.254.169.254/")
        .expect_err("should reject cloud metadata even with https");
    assert!(err.contains("blocked host"), "unexpected error: {err}");
}

#[test]
fn validate_url_allows_https_with_path() {
    assert!(validate_url("https://api.example.com/v1/tools").is_ok());
}

#[test]
fn validate_url_allows_http_localhost_without_port() {
    assert!(validate_url("http://localhost/").is_ok());
}

#[test]
fn validate_url_allows_http_127_0_0_1_without_port() {
    assert!(validate_url("http://127.0.0.1/").is_ok());
}
