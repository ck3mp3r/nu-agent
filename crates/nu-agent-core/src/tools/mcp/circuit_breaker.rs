use std::collections::HashMap;

/// Circuit breaker for MCP transport failures.
///
/// Tracks consecutive transport errors per MCP server prefix. When the
/// failure count reaches the configured threshold, the circuit "trips" —
/// signalling the caller to disable the server.
///
/// A single successful tool call resets the counter for that server.
pub struct McpCircuitBreaker {
    /// Consecutive failure count per server prefix.
    failure_counts: HashMap<String, usize>,
    /// Number of consecutive failures before disabling a server.
    threshold: usize,
}

/// Default number of consecutive transport failures before tripping.
const DEFAULT_THRESHOLD: usize = 3;

impl McpCircuitBreaker {
    /// Create a new circuit breaker with the given threshold.
    ///
    /// A threshold of 0 is treated as the default (3).
    pub fn new(threshold: usize) -> Self {
        Self {
            failure_counts: HashMap::new(),
            threshold: if threshold == 0 {
                DEFAULT_THRESHOLD
            } else {
                threshold
            },
        }
    }

    /// Record a transport failure for `server_prefix`.
    ///
    /// Returns `true` when the failure count is at or above the threshold (the
    /// circuit is tripped and the server should remain disabled).
    pub fn record_failure(&mut self, server_prefix: &str) -> bool {
        let count = self
            .failure_counts
            .entry(server_prefix.to_string())
            .or_default();
        *count += 1;
        *count >= self.threshold
    }

    /// Record a successful tool call for `server_prefix`, resetting its
    /// failure counter.
    pub fn record_success(&mut self, server_prefix: &str) {
        self.failure_counts.remove(server_prefix);
    }

    /// Returns the current threshold.
    pub fn threshold(&self) -> usize {
        self.threshold
    }
}

impl Default for McpCircuitBreaker {
    fn default() -> Self {
        Self::new(DEFAULT_THRESHOLD)
    }
}

/// Returns `true` if the text looks like an MCP transport failure.
///
/// Matches (case-insensitive):
/// - "Transport closed"
/// - "transport error"
/// - "connection refused"
pub fn is_transport_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("transport closed")
        || lower.contains("transport error")
        || lower.contains("connection refused")
}

#[cfg(test)]
#[path = "circuit_breaker_test.rs"]
mod circuit_breaker_test;
