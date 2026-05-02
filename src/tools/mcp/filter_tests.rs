use super::matches_patterns;

#[test]
fn matches_patterns_empty_matches_all() {
    assert!(matches_patterns("k8s__list_pods", &[]));
}

#[test]
fn matches_patterns_single_glob() {
    let patterns = vec!["k8s__*".to_string()];
    assert!(matches_patterns("k8s__list_pods", &patterns));
    assert!(!matches_patterns("gh__list_prs", &patterns));
}

#[test]
fn matches_patterns_multiple_globs_or_semantics() {
    let patterns = vec!["k8s__*".to_string(), "gh__list_*".to_string()];
    assert!(matches_patterns("k8s__get_pod", &patterns));
    assert!(matches_patterns("gh__list_prs", &patterns));
    assert!(!matches_patterns("git__status", &patterns));
}

#[test]
fn matches_patterns_case_behavior_documented() {
    // Case-sensitive by design.
    let patterns = vec!["K8S__*".to_string()];
    assert!(!matches_patterns("k8s__list_pods", &patterns));
}
