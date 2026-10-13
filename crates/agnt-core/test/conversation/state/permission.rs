use super::*;

use crate::tools::authz::{PermissionAction, PermissionDecision, PermissionRuleMatch};

/// Tool-call arguments as rig 0.44 carries them: a JSON object map.
macro_rules! args_map {
    ($($json:tt)*) => {
        serde_json::json!($($json)*)
            .as_object()
            .cloned()
            .unwrap_or_default()
    };
}

fn global_ask_decision() -> PermissionDecision {
    PermissionDecision {
        action: PermissionAction::Ask,
        matched_rule: PermissionRuleMatch {
            identity: "global:*".to_string(),
            scope: "global",
            target_field: None,
            pattern: "*".to_string(),
            action: PermissionAction::Ask,
        },
        diagnostics: Vec::new(),
    }
}

fn permission_state_with_grants() -> PermissionState {
    let config = PermissionsConfig::safe_defaults(true);
    let mut cache = SessionGrantCache::default();

    // Insert a grant directly into the cache
    let decision = global_ask_decision();
    cache.insert_allow_always(
        &decision,
        "shell",
        "closure",
        &args_map!({"command": "echo test"}),
    );

    PermissionState::new(
        config.clone(),
        config,
        None,
        cache,
        "test state".to_string(),
    )
}

#[test]
fn clear_session_grants_empties_cache() {
    let state = permission_state_with_grants();

    // Verify grant exists before clear
    {
        let cache = state.session_grants.lock().expect("test lock");
        let decision = global_ask_decision();
        assert_eq!(
            cache.get(
                &decision,
                "shell",
                "closure",
                &args_map!({"command": "echo test"})
            ),
            Some(PermissionAction::Allow)
        );
    }

    // Clear all session grants
    state.clear_session_grants();

    // Verify grants are gone
    {
        let cache = state.session_grants.lock().expect("test lock");
        let decision = global_ask_decision();
        assert_eq!(
            cache.get(
                &decision,
                "shell",
                "closure",
                &args_map!({"command": "echo test"})
            ),
            None
        );
    }
}

#[test]
fn clear_session_grants_on_empty_state() {
    let config = PermissionsConfig::safe_defaults(true);
    let state = PermissionState::new(
        config.clone(),
        config,
        None,
        SessionGrantCache::default(),
        "test state".to_string(),
    );

    // Should not panic
    state.clear_session_grants();

    // Verify still empty
    let cache = state.session_grants.lock().expect("test lock");
    let decision = global_ask_decision();
    assert_eq!(
        cache.get(
            &decision,
            "shell",
            "closure",
            &args_map!({"command": "echo test"})
        ),
        None
    );
}

#[test]
fn clear_session_grants_for_server_removes_only_matching() {
    let config = PermissionsConfig::safe_defaults(true);
    let mut cache = SessionGrantCache::default();

    // Insert grants for context7__search and gh__list_prs
    let context7_decision = global_ask_decision();
    cache.insert_allow_always(
        &context7_decision,
        "context7__search",
        "mcp",
        &args_map!({"query": "test"}),
    );

    let gh_decision = global_ask_decision();
    cache.insert_allow_always(&gh_decision, "gh__list_prs", "mcp", &args_map!({}));

    let state = PermissionState::new(
        config.clone(),
        config,
        None,
        cache,
        "test state".to_string(),
    );

    // Verify both grants exist before clear
    {
        let cache = state.session_grants.lock().expect("test lock");
        let decision = global_ask_decision();
        assert_eq!(
            cache.get(
                &decision,
                "context7__search",
                "mcp",
                &args_map!({"query": "test"})
            ),
            Some(PermissionAction::Allow)
        );
        assert_eq!(
            cache.get(&decision, "gh__list_prs", "mcp", &args_map!({})),
            Some(PermissionAction::Allow)
        );
    }

    // Clear grants for context7 server
    state.clear_session_grants_for_server("context7");

    // Verify context7__search is removed, gh__list_prs remains
    {
        let cache = state.session_grants.lock().expect("test lock");
        let decision = global_ask_decision();
        assert_eq!(
            cache.get(
                &decision,
                "context7__search",
                "mcp",
                &args_map!({"query": "test"})
            ),
            None
        );
        assert_eq!(
            cache.get(&decision, "gh__list_prs", "mcp", &args_map!({})),
            Some(PermissionAction::Allow)
        );
    }
}

#[test]
fn clear_session_grants_for_server_on_empty_state() {
    let config = PermissionsConfig::safe_defaults(true);
    let state = PermissionState::new(
        config.clone(),
        config,
        None,
        SessionGrantCache::default(),
        "test state".to_string(),
    );

    // Should not panic
    state.clear_session_grants_for_server("context7");

    // Verify still empty
    let cache = state.session_grants.lock().expect("test lock");
    let decision = global_ask_decision();
    assert_eq!(
        cache.get(
            &decision,
            "shell",
            "closure",
            &args_map!({"command": "echo test"})
        ),
        None
    );
}
