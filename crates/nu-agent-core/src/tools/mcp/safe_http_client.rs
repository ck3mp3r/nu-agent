//! URL validation for SSRF protection.
//!
//! Provides [`validate_url`] which checks that a URL is safe to make HTTP
//! requests to. This is used to prevent SSRF attacks by:
//!
//! - Requiring HTTPS (except localhost/127.0.0.1)
//! - Blocking cloud metadata endpoints (169.254.169.254)
//! - Blocking link-local addresses (169.254.0.0/16)

use url::Url;

/// Validate that `url` is safe to make an HTTP request to.
///
/// Returns `Ok(())` if the URL passes all security checks, or `Err(String)`
/// with a human-readable explanation of why the URL was rejected.
///
/// # Security checks
///
/// 1. **HTTPS required** — only `https` scheme is allowed, except for
///    `localhost` and `127.0.0.1` which may use `http`.
/// 2. **Cloud metadata blocked** — `169.254.169.254` is the AWS/GCP/Azure
///    instance metadata endpoint and is always blocked.
/// 3. **Link-local blocked** — the entire `169.254.0.0/16` range is reserved
///    for link-local addressing and is blocked.
pub fn validate_url(url: &str) -> Result<(), String> {
    let parsed: Url = Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;

    // HTTPS only (except localhost/127.0.0.1)
    if parsed.scheme() != "https"
        && parsed.host_str() != Some("localhost")
        && parsed.host_str() != Some("127.0.0.1")
    {
        return Err(format!(
            "insecure URL scheme: {} (HTTPS required)",
            parsed.scheme()
        ));
    }

    // Block cloud metadata endpoints
    if parsed.host_str() == Some("169.254.169.254") {
        return Err("blocked host: cloud metadata endpoint".to_string());
    }

    // Block link-local addresses (169.254.0.0/16)
    if let Some(host) = parsed.host_str()
        && let Ok(ip) = host.parse::<std::net::Ipv4Addr>()
        && ip.is_link_local()
    {
        return Err(format!("blocked host: link-local address {ip}"));
    }

    Ok(())
}

#[cfg(test)]
#[path = "safe_http_client_test.rs"]
mod safe_http_client_test;
