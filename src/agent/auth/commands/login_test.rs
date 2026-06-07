use super::AgentAuthLogin;
use nu_plugin::SimplePluginCommand;
use nu_protocol::SyntaxShape;

#[test]
fn login_command_name() {
    let command = AgentAuthLogin::new();
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent auth login",
        "Command name should be 'agent auth login'"
    );
}

#[test]
fn login_signature_has_provider_flag() {
    let command = AgentAuthLogin::new();
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(sig.name, "agent auth login", "Signature name should match");

    // Check for --provider/-p flag
    let provider_flag = sig
        .named
        .iter()
        .find(|f| f.long == "provider")
        .expect("Should have --provider flag");

    assert_eq!(provider_flag.short, Some('p'), "Should have -p short form");

    // Check that the flag accepts a String
    match provider_flag.arg {
        Some(SyntaxShape::String) => (),
        _ => panic!("--provider flag should accept String argument"),
    }
}

#[test]
fn login_description_is_meaningful() {
    let command = AgentAuthLogin::new();
    let description = SimplePluginCommand::description(&command);

    assert!(!description.is_empty(), "Description should not be empty");
    assert!(
        description.contains("provider") || description.contains("Authenticate"),
        "Description should mention provider or authentication"
    );
}
