use super::validate_url;

#[test]
fn validate_url_allows_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str)> = vec![
        ("https mcp example", "https://mcp.example.com/"),
        ("http localhost with port", "http://localhost:8080/"),
        ("http 127.0.0.1 with port", "http://127.0.0.1:8080/"),
        ("https with path", "https://api.example.com/v1/tools"),
        ("http localhost without port", "http://localhost/"),
        ("http 127.0.0.1 without port", "http://127.0.0.1/"),
    ];

    // -- Exec & Check
    for (name, url) in cases {
        assert!(
            validate_url(url).is_ok(),
            "case '{name}': {url} must be allowed"
        );
    }
}

#[test]
fn validate_url_rejects_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &str)> = vec![
        (
            "http cloud metadata",
            "http://169.254.169.254/",
            "insecure URL scheme",
        ),
        (
            "http internal service",
            "http://internal-service/",
            "insecure URL scheme",
        ),
        (
            "http link local",
            "http://169.254.1.1/",
            "insecure URL scheme",
        ),
        ("https link local", "https://169.254.1.1/", "blocked host"),
        (
            "https cloud metadata",
            "https://169.254.169.254/",
            "blocked host",
        ),
        ("invalid url", "not a url", "invalid URL"),
    ];

    // -- Exec & Check
    for (name, url, expected_fragment) in cases {
        let err = validate_url(url).expect_err("this url must be rejected");
        assert!(
            err.contains(expected_fragment),
            "case '{name}': error must contain '{expected_fragment}', got: {err}"
        );
    }
}
