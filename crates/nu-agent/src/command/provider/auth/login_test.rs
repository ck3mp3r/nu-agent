use super::AgentProviderAuthLogin;
use nu_plugin::SimplePluginCommand;
use nu_protocol::SyntaxShape;

#[test]
fn login_command_name() {
    let command = AgentProviderAuthLogin::new();
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent provider auth login",
        "Command name should be 'agent provider auth login'"
    );
}

#[test]
fn login_signature_has_name_arg() {
    let command = AgentProviderAuthLogin::new();
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(
        sig.name, "agent provider auth login",
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

#[test]
fn login_signature_has_api_key_flag() {
    let command = AgentProviderAuthLogin::new();
    let sig = SimplePluginCommand::signature(&command);

    let api_key_flag = sig
        .named
        .iter()
        .find(|f| f.long == "api-key")
        .expect("Should have --api-key flag");

    match api_key_flag.arg {
        Some(SyntaxShape::String) => (),
        _ => panic!("--api-key flag should accept String argument"),
    }
}
