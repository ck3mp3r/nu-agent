use super::Error;

#[test]
fn error_invalid_provider_format() {
    let err = Error::InvalidProviderFormat("github-copilot/anthropic".to_string());
    let msg = err.to_string();
    assert!(
        msg.contains("Invalid provider format"),
        "Error should mention invalid format, got: {}",
        msg
    );
    assert!(
        msg.contains("github-copilot/anthropic"),
        "Error should show received value, got: {}",
        msg
    );
}

#[test]
fn error_missing_api_key() {
    let err = Error::MissingApiKey;
    assert!(err.to_string().contains("API key"));
}

#[test]
fn error_unknown_backend() {
    let err = Error::UnknownBackend("foo".to_string());
    assert!(err.to_string().contains("foo"));
}
