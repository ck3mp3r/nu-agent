use super::AgentProviderAuthLogout;
use nu_plugin::SimplePluginCommand;
use nu_protocol::SyntaxShape;

#[test]
fn logout_command_name() {
    let command = AgentProviderAuthLogout::new();
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent provider auth logout",
        "Command name should be 'agent provider auth logout'"
    );
}

#[test]
fn logout_signature_has_name_arg() {
    let command = AgentProviderAuthLogout::new();
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(
        sig.name, "agent provider auth logout",
        "Signature name should match"
    );

    let name_arg = sig
        .required_positional
        .iter()
        .find(|f| f.name == "name")
        .expect("Should have required positional 'name' arg");

    match name_arg.shape {
        SyntaxShape::String => (),
        _ => panic!("'name' arg should accept String argument"),
    }
}
