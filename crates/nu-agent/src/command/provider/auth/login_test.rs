use super::AgentProviderAuthLogin;
use nu_plugin::SimplePluginCommand;
use nu_protocol::SyntaxShape;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn login_command_name() {
    let command = AgentProviderAuthLogin;
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent provider auth login",
        "Command name should be 'agent provider auth login'"
    );
}

#[test]
fn login_signature_has_name_arg() -> Result<()> {
    let command = AgentProviderAuthLogin;
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(
        sig.name, "agent provider auth login",
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

#[test]
fn login_signature_has_api_key_flag() -> Result<()> {
    let command = AgentProviderAuthLogin;
    let sig = SimplePluginCommand::signature(&command);

    let api_key_flag = sig
        .named
        .iter()
        .find(|f| f.long == "api-key")
        .ok_or("should have --api-key flag")?;

    match api_key_flag.arg {
        Some(SyntaxShape::String) => (),
        _ => panic!("--api-key flag should accept String argument"),
    }
    Ok(())
}
