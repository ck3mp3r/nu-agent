use mdns_sd::{IntoTxtProperties, TxtProperties};

use super::filter::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a [`TxtProperties`] from a slice of key-value string pairs.
fn make_txt_props(props: &[(&str, &str)]) -> TxtProperties {
    props.into_txt_properties()
}

// ---------------------------------------------------------------------------
// check_mesh_key_filter
// ---------------------------------------------------------------------------

#[test]
fn test_mesh_key_filter_rejects_different_key() {
    let props = make_txt_props(&[("mesh_key", "key-a")]);
    let result = check_mesh_key_filter("key-b", &props);
    assert!(
        matches!(
            result,
            FilterDecision::Reject(FilterRejection::MeshKeyMismatch { .. })
        ),
        "expected MeshKeyMismatch rejection, got {result:?}"
    );
}

#[test]
fn test_mesh_key_filter_accepts_matching_key() {
    let props = make_txt_props(&[("mesh_key", "key-a")]);
    let result = check_mesh_key_filter("key-a", &props);
    assert_eq!(result, FilterDecision::Accept);
}

#[test]
fn test_mesh_key_filter_rejects_missing_key() {
    let props = TxtProperties::new();
    let result = check_mesh_key_filter("any", &props);
    assert!(
        matches!(
            result,
            FilterDecision::Reject(FilterRejection::MissingMeshKey { .. })
        ),
        "expected MissingMeshKey rejection, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// check_all (integration of the filter chain)
// ---------------------------------------------------------------------------

#[test]
fn test_check_all_self_port_takes_precedence() {
    let filters = build_filters(52095, "key-a");
    let props = make_txt_props(&[("mesh_key", "key-b")]);
    let result = check_all(
        &filters,
        "any-agent._nu-agent-a2a._tcp.local.",
        &props,
        52095,
        true,
    );
    assert!(
        matches!(result, Err(FilterRejection::SelfPortMatch { port: 52095 })),
        "expected SelfPortMatch (first filter), got {result:?}"
    );
}

#[test]
fn test_check_all_rejects_mesh_key_mismatch() {
    let filters = build_filters(52095, "key-a");
    let props = make_txt_props(&[("mesh_key", "key-b")]);
    let result = check_all(
        &filters,
        "other-agent._nu-agent-a2a._tcp.local.",
        &props,
        52096,
        false,
    );
    assert!(
        matches!(result, Err(FilterRejection::MeshKeyMismatch { .. })),
        "expected MeshKeyMismatch rejection, got {result:?}"
    );
}

#[test]
fn test_check_all_accepts_matching_peer() {
    let filters = build_filters(52095, "key-a");
    let props = make_txt_props(&[("mesh_key", "key-a")]);
    let result = check_all(
        &filters,
        "other-agent._nu-agent-a2a._tcp.local.",
        &props,
        52096,
        false,
    );
    assert!(result.is_ok(), "expected Ok, got Err({result:?})");
}

// ---------------------------------------------------------------------------
// build_filters
// ---------------------------------------------------------------------------

#[test]
fn test_build_filters_order() {
    let filters = build_filters(52095, "my-key");
    assert_eq!(filters.len(), 2);
    assert!(
        matches!(&filters[0], Filter::SelfPort(_)),
        "first filter should be SelfPort, got {:?}",
        filters[0]
    );
    assert!(
        matches!(&filters[1], Filter::MeshKey(_)),
        "second filter should be MeshKey, got {:?}",
        filters[1]
    );
}

// ---------------------------------------------------------------------------
// Display formatting
// ---------------------------------------------------------------------------

#[test]
fn test_filter_rejection_display_mesh_key_mismatch() {
    let rejection = FilterRejection::MeshKeyMismatch {
        peer_key: "key-a".to_string(),
        our_key: "key-b".to_string(),
    };
    let msg = rejection.to_string();
    assert!(
        msg.contains("mesh key mismatch"),
        "unexpected display: {msg}"
    );
}

#[test]
fn test_filter_rejection_display_missing_mesh_key() {
    let rejection = FilterRejection::MissingMeshKey {
        fullname: "peer._nu-agent-a2a._tcp.local.".to_string(),
    };
    let msg = rejection.to_string();
    assert!(
        msg.contains("missing mesh_key"),
        "unexpected display: {msg}"
    );
}

#[test]
fn test_filter_rejection_display_self_port_match() {
    let rejection = FilterRejection::SelfPortMatch { port: 52095 };
    let msg = rejection.to_string();
    assert!(
        msg.contains("self-match (port 52095 with localhost)"),
        "unexpected display: {msg}"
    );
}

// ---------------------------------------------------------------------------
// check_self_port_filter
// ---------------------------------------------------------------------------

#[test]
fn test_self_port_filter_rejects_localhost_match() {
    let result = check_self_port_filter(52095, 52095, true);
    assert!(
        matches!(
            result,
            FilterDecision::Reject(FilterRejection::SelfPortMatch { port: 52095 })
        ),
        "expected SelfPortMatch rejection, got {result:?}"
    );
}

#[test]
fn test_self_port_filter_accepts_different_port() {
    let result = check_self_port_filter(52095, 52096, true);
    assert_eq!(result, FilterDecision::Accept);
}

#[test]
fn test_self_port_filter_accepts_no_localhost() {
    let result = check_self_port_filter(52095, 52095, false);
    assert_eq!(result, FilterDecision::Accept);
}

#[test]
fn test_self_port_filter_accepts_different_port_no_localhost() {
    let result = check_self_port_filter(52095, 52096, false);
    assert_eq!(result, FilterDecision::Accept);
}
