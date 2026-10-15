use super::*;

#[test]
fn test_utility_config_generate_config_content_substitutes_placeholder() {
    // -- Exec
    let content = generate_config_content();

    // -- Check
    assert!(
        !content.contains("{{ACTIVE_MODEL}}"),
        "the active-model placeholder must be substituted"
    );
    assert!(content.contains("# nu-agent configuration"));
}
