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
}

#[test]
fn from_str_returns_none_for_invalid_string() {
    assert!(BuiltinKind::from_str("foo").is_err());
    assert!(BuiltinKind::from_str("").is_err());
    assert!(BuiltinKind::from_str("READ").is_err());
}
