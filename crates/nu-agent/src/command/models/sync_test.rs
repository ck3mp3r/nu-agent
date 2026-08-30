use super::AgentModelsSync;
use nu_plugin::SimplePluginCommand;

#[test]
fn sync_command_name() {
    let command = AgentModelsSync;
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent models sync",
        "Command name should be 'agent models sync'"
    );
}

#[test]
fn sync_signature_has_no_required_args() {
    let command = AgentModelsSync;
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(sig.name, "agent models sync", "Signature name should match");
    assert!(
        sig.required_positional.is_empty(),
        "sync command should take no positional args"
    );
}
