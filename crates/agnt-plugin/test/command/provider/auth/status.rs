use super::AgentProviderAuthStatus;
use nu_plugin::SimplePluginCommand;

#[test]
fn status_command_name() {
    let command = AgentProviderAuthStatus;
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent provider auth status",
        "Command name should be 'agent provider auth status'"
    );
}

#[test]
fn status_signature_has_no_positional_args() {
    let command = AgentProviderAuthStatus;
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(
        sig.name, "agent provider auth status",
        "Signature name should match"
    );
    assert!(
        sig.required_positional.is_empty(),
        "status command should take no positional args"
    );
    assert!(
        sig.optional_positional.is_empty(),
        "status command should take no optional positional args"
    );
}
