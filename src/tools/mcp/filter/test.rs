use super::matches_patterns;

#[test]
fn matches_patterns_empty_matches_all() {
    assert!(matches_patterns("k8s/list_pods", &[]));
}

#[test]
fn matches_patterns_single_glob() {
    let patterns = vec!["k8s/*".to_string()];
    assert!(matches_patterns("k8s/list_pods", &patterns));
    assert!(!matches_patterns("gh/list_prs", &patterns));
}

#[test]
fn matches_patterns_multiple_globs_or_semantics() {
    let patterns = vec!["k8s/*".to_string(), "gh/list_*".to_string()];
    assert!(matches_patterns("k8s/get_pod", &patterns));
    assert!(matches_patterns("gh/list_prs", &patterns));
    assert!(!matches_patterns("git/status", &patterns));
}

#[test]
fn matches_patterns_case_behavior_documented() {
    // Case-sensitive by design.
    let patterns = vec!["K8S/*".to_string()];
    assert!(!matches_patterns("k8s/list_pods", &patterns));
}
