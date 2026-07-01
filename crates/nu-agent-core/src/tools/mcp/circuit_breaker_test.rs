use super::*;

#[test]
fn circuit_breaker_trips_after_threshold() {
    let mut cb = McpCircuitBreaker::new(3);

    assert!(!cb.record_failure("server_a"));
    assert!(!cb.record_failure("server_a"));
    // 3rd failure trips the breaker
    assert!(cb.record_failure("server_a"));
    assert_eq!(*cb.failure_counts.get("server_a").unwrap(), 3);
}

#[test]
fn circuit_breaker_resets_on_success() {
    let mut cb = McpCircuitBreaker::new(3);

    // 2 failures
    assert!(!cb.record_failure("server_a"));
    assert!(!cb.record_failure("server_a"));

    // 1 success resets
    cb.record_success("server_a");
    assert!(!cb.failure_counts.contains_key("server_a"));

    // 2 more failures — never reaches threshold
    assert!(!cb.record_failure("server_a"));
    assert!(!cb.record_failure("server_a"));
    assert_eq!(*cb.failure_counts.get("server_a").unwrap(), 2);
}

#[test]
fn circuit_breaker_independent_per_server() {
    let mut cb = McpCircuitBreaker::new(3);

    // 2 failures on server_a — does NOT trip
    assert!(!cb.record_failure("server_a"));
    assert!(!cb.record_failure("server_a"));

    // 3 failures on server_b — trips only server_b
    assert!(!cb.record_failure("server_b"));
    assert!(!cb.record_failure("server_b"));
    assert!(cb.record_failure("server_b"));

    // server_a still at 2
    assert_eq!(*cb.failure_counts.get("server_a").unwrap(), 2);
    assert_eq!(*cb.failure_counts.get("server_b").unwrap(), 3);
}

#[test]
fn default_threshold_is_three() {
    let cb = McpCircuitBreaker::default();
    assert_eq!(cb.threshold(), 3);
}

#[test]
fn zero_threshold_uses_default() {
    let cb = McpCircuitBreaker::new(0);
    assert_eq!(cb.threshold(), 3);
}

#[test]
fn custom_threshold_is_respected() {
    let mut cb = McpCircuitBreaker::new(5);
    assert_eq!(cb.threshold(), 5);

    for _ in 0..4 {
        assert!(!cb.record_failure("server_a"));
    }
    // 5th failure trips
    assert!(cb.record_failure("server_a"));
}

#[test]
fn trips_stay_tripped_on_further_failures() {
    let mut cb = McpCircuitBreaker::new(3);
    for _ in 0..3 {
        cb.record_failure("server_a");
    }
    // 4th and 5th failures still return true
    assert!(cb.record_failure("server_a"));
    assert!(cb.record_failure("server_a"));
}

// --- is_transport_error tests ---

#[test]
fn detects_transport_closed() {
    assert!(is_transport_error("Transport closed"));
    assert!(is_transport_error("Toolset error: Transport closed"));
    assert!(is_transport_error("TRANSPORT CLOSED"));
}

#[test]
fn detects_transport_error() {
    assert!(is_transport_error("transport error: broken pipe"));
    assert!(is_transport_error("Transport Error occurred"));
}

#[test]
fn detects_connection_refused() {
    assert!(is_transport_error("connection refused"));
    assert!(is_transport_error("Connection Refused on port 8080"));
}

#[test]
fn does_not_match_normal_results() {
    assert!(!is_transport_error("Success"));
    assert!(!is_transport_error("file not found"));
    assert!(!is_transport_error("permission denied"));
    assert!(!is_transport_error(""));
}
