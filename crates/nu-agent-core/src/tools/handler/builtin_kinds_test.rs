use super::BuiltinKind;
use std::str::FromStr;

#[test]
fn as_str_returns_correct_string_for_all_variants() {
    assert_eq!(BuiltinKind::Read.as_str(), "read");
    assert_eq!(BuiltinKind::Edit.as_str(), "edit");
    assert_eq!(BuiltinKind::Patch.as_str(), "patch");
    assert_eq!(BuiltinKind::Skill.as_str(), "skill");
    assert_eq!(BuiltinKind::SpawnAgent.as_str(), "spawn_agent");
    assert_eq!(BuiltinKind::TerminateAgent.as_str(), "terminate_agent");
    assert_eq!(BuiltinKind::SendMessage.as_str(), "send_message");
    assert_eq!(BuiltinKind::ListAgents.as_str(), "list_agents");
    assert_eq!(BuiltinKind::Http.as_str(), "http");
    assert_eq!(BuiltinKind::Grep.as_str(), "grep");
    assert_eq!(BuiltinKind::Glob.as_str(), "glob");
    assert_eq!(BuiltinKind::Nu.as_str(), "nu");
    assert_eq!(BuiltinKind::TmuxSession.as_str(), "tmux_session");
    assert_eq!(BuiltinKind::TmuxWindow.as_str(), "tmux_window");
    assert_eq!(BuiltinKind::TmuxPane.as_str(), "tmux_pane");
    assert_eq!(BuiltinKind::TmuxLayout.as_str(), "tmux_layout");
}

#[test]
fn from_str_returns_some_for_all_valid_strings() {
    assert_eq!(BuiltinKind::from_str("read"), Ok(BuiltinKind::Read));
    assert_eq!(BuiltinKind::from_str("edit"), Ok(BuiltinKind::Edit));
    assert_eq!(BuiltinKind::from_str("patch"), Ok(BuiltinKind::Patch));
    assert_eq!(BuiltinKind::from_str("skill"), Ok(BuiltinKind::Skill));
    assert_eq!(
        BuiltinKind::from_str("spawn_agent"),
        Ok(BuiltinKind::SpawnAgent)
    );
    assert_eq!(
        BuiltinKind::from_str("terminate_agent"),
        Ok(BuiltinKind::TerminateAgent)
    );
    assert_eq!(
        BuiltinKind::from_str("send_message"),
        Ok(BuiltinKind::SendMessage)
    );
    assert_eq!(
        BuiltinKind::from_str("list_agents"),
        Ok(BuiltinKind::ListAgents)
    );
    assert_eq!(BuiltinKind::from_str("http"), Ok(BuiltinKind::Http));
    assert_eq!(BuiltinKind::from_str("grep"), Ok(BuiltinKind::Grep));
    assert_eq!(BuiltinKind::from_str("glob"), Ok(BuiltinKind::Glob));
    assert_eq!(BuiltinKind::from_str("nu"), Ok(BuiltinKind::Nu));
    assert_eq!(
        BuiltinKind::from_str("tmux_session"),
        Ok(BuiltinKind::TmuxSession)
    );
    assert_eq!(
        BuiltinKind::from_str("tmux_window"),
        Ok(BuiltinKind::TmuxWindow)
    );
    assert_eq!(
        BuiltinKind::from_str("tmux_pane"),
        Ok(BuiltinKind::TmuxPane)
    );
    assert_eq!(
        BuiltinKind::from_str("tmux_layout"),
        Ok(BuiltinKind::TmuxLayout)
    );
}

#[test]
fn from_str_round_trips_all_tmux_variants() {
    for kind in [
        BuiltinKind::TmuxSession,
        BuiltinKind::TmuxWindow,
        BuiltinKind::TmuxPane,
        BuiltinKind::TmuxLayout,
    ] {
        assert_eq!(BuiltinKind::from_str(kind.as_str()), Ok(kind));
    }
}

#[test]
fn from_str_returns_none_for_invalid_string() {
    assert!(BuiltinKind::from_str("foo").is_err());
    assert!(BuiltinKind::from_str("").is_err());
    assert!(BuiltinKind::from_str("READ").is_err());
}
