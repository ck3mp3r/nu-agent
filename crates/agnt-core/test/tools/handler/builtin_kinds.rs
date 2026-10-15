use super::BuiltinKind;
use std::str::FromStr;

#[test]
fn kind_string_round_trip_table() {
    // -- Setup & Fixtures
    let cases: Vec<(BuiltinKind, &str)> = vec![
        (BuiltinKind::Read, "read"),
        (BuiltinKind::Edit, "edit"),
        (BuiltinKind::Skill, "skill"),
        (BuiltinKind::SpawnAgent, "spawn_agent"),
        (BuiltinKind::TerminateAgent, "terminate_agent"),
        (BuiltinKind::SendMessage, "send_message"),
        (BuiltinKind::ListAgents, "list_agents"),
        (BuiltinKind::Http, "http"),
        (BuiltinKind::Grep, "grep"),
        (BuiltinKind::Glob, "glob"),
        (BuiltinKind::Nu, "nu"),
        (BuiltinKind::TmuxSession, "tmux_session"),
        (BuiltinKind::TmuxWindow, "tmux_window"),
        (BuiltinKind::TmuxPane, "tmux_pane"),
        (BuiltinKind::TmuxLayout, "tmux_layout"),
        (BuiltinKind::AstQuery, "ast_query"),
        (BuiltinKind::AstNodes, "ast_nodes"),
        (BuiltinKind::AstRefs, "ast_refs"),
        (BuiltinKind::AstTree, "ast_tree"),
    ];

    // -- Exec & Check
    for (kind, expected) in cases {
        assert_eq!(kind.as_str(), expected, "as_str for {kind:?}");
        assert_eq!(
            BuiltinKind::from_str(expected),
            Ok(kind),
            "from_str must round trip '{expected}'"
        );
    }
}

#[test]
fn from_str_returns_none_for_invalid_string() {
    assert!(BuiltinKind::from_str("foo").is_err());
    assert!(BuiltinKind::from_str("").is_err());
    assert!(BuiltinKind::from_str("READ").is_err());
}
