use super::AgentProviderAuthLogout;
use nu_plugin::SimplePluginCommand;
use nu_protocol::SyntaxShape;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn logout_command_name() {
    let command = AgentProviderAuthLogout;
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent provider auth logout",
        "Command name should be 'agent provider auth logout'"
    );
}

#[test]
fn logout_signature_has_name_arg() -> Result<()> {
    let command = AgentProviderAuthLogout;
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(
        sig.name, "agent provider auth logout",
        "Signature name should match"
    );

    let name_arg = sig
        .required_positional
        .iter()
        .find(|f| f.name == "name")
        .ok_or("should have required positional 'name' arg")?;

    match name_arg.shape {
        SyntaxShape::String => (),
        _ => panic!("'name' arg should accept String argument"),
    }
    Ok(())
}
