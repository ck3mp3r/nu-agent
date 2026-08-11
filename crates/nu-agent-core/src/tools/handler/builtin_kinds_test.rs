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
fn is_fs_true_only_for_edit_and_patch() {
    assert!(BuiltinKind::Edit.is_fs());
    assert!(BuiltinKind::Patch.is_fs());

    assert!(!BuiltinKind::Read.is_fs());
    assert!(!BuiltinKind::Skill.is_fs());
    assert!(!BuiltinKind::SpawnAgent.is_fs());
    assert!(!BuiltinKind::TerminateAgent.is_fs());
    assert!(!BuiltinKind::SendMessage.is_fs());
    assert!(!BuiltinKind::ListAgents.is_fs());
    assert!(!BuiltinKind::Http.is_fs());
    assert!(!BuiltinKind::Grep.is_fs());
    assert!(!BuiltinKind::Glob.is_fs());
    assert!(!BuiltinKind::Nu.is_fs());
    assert!(!BuiltinKind::TmuxSession.is_fs());
    assert!(!BuiltinKind::TmuxWindow.is_fs());
    assert!(!BuiltinKind::TmuxPane.is_fs());
    assert!(!BuiltinKind::TmuxLayout.is_fs());
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
fn is_tmux_true_only_for_tmux_variants() {
    assert!(BuiltinKind::TmuxSession.is_tmux());
    assert!(BuiltinKind::TmuxWindow.is_tmux());
    assert!(BuiltinKind::TmuxPane.is_tmux());
    assert!(BuiltinKind::TmuxLayout.is_tmux());

    assert!(!BuiltinKind::Read.is_tmux());
    assert!(!BuiltinKind::Edit.is_tmux());
    assert!(!BuiltinKind::Patch.is_tmux());
    assert!(!BuiltinKind::Skill.is_tmux());
    assert!(!BuiltinKind::SpawnAgent.is_tmux());
    assert!(!BuiltinKind::TerminateAgent.is_tmux());
    assert!(!BuiltinKind::SendMessage.is_tmux());
    assert!(!BuiltinKind::ListAgents.is_tmux());
    assert!(!BuiltinKind::Http.is_tmux());
    assert!(!BuiltinKind::Grep.is_tmux());
    assert!(!BuiltinKind::Glob.is_tmux());
    assert!(!BuiltinKind::Nu.is_tmux());
}

#[test]
fn is_privileged_true_for_edit_patch_tmux_and_nu_variants() {
    assert!(BuiltinKind::Edit.is_privileged());
    assert!(BuiltinKind::Patch.is_privileged());
    assert!(BuiltinKind::TmuxSession.is_privileged());
    assert!(BuiltinKind::TmuxWindow.is_privileged());
    assert!(BuiltinKind::TmuxPane.is_privileged());
    assert!(BuiltinKind::TmuxLayout.is_privileged());
    assert!(BuiltinKind::Nu.is_privileged());

    assert!(!BuiltinKind::Read.is_privileged());
    assert!(!BuiltinKind::Skill.is_privileged());
    assert!(!BuiltinKind::SpawnAgent.is_privileged());
    assert!(!BuiltinKind::TerminateAgent.is_privileged());
    assert!(!BuiltinKind::SendMessage.is_privileged());
    assert!(!BuiltinKind::ListAgents.is_privileged());
    assert!(!BuiltinKind::Http.is_privileged());
    assert!(!BuiltinKind::Grep.is_privileged());
    assert!(!BuiltinKind::Glob.is_privileged());
}

#[test]
fn is_nu_true_only_for_nu() {
    assert!(BuiltinKind::Nu.is_nu());

    assert!(!BuiltinKind::Read.is_nu());
    assert!(!BuiltinKind::Edit.is_nu());
    assert!(!BuiltinKind::Patch.is_nu());
    assert!(!BuiltinKind::Skill.is_nu());
    assert!(!BuiltinKind::SpawnAgent.is_nu());
    assert!(!BuiltinKind::TerminateAgent.is_nu());
    assert!(!BuiltinKind::SendMessage.is_nu());
    assert!(!BuiltinKind::ListAgents.is_nu());
    assert!(!BuiltinKind::Http.is_nu());
    assert!(!BuiltinKind::Grep.is_nu());
    assert!(!BuiltinKind::Glob.is_nu());
    assert!(!BuiltinKind::TmuxSession.is_nu());
    assert!(!BuiltinKind::TmuxWindow.is_nu());
    assert!(!BuiltinKind::TmuxPane.is_nu());
    assert!(!BuiltinKind::TmuxLayout.is_nu());
}

#[test]
fn from_str_returns_none_for_invalid_string() {
    assert!(BuiltinKind::from_str("foo").is_err());
    assert!(BuiltinKind::from_str("").is_err());
    assert!(BuiltinKind::from_str("READ").is_err());
}
