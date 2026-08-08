use super::AgentModelsList;
use nu_plugin::SimplePluginCommand;
use nu_protocol::SyntaxShape;

#[test]
fn list_command_name() {
    let command = AgentModelsList::new();
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent models list",
        "Command name should be 'agent models list'"
    );
}

#[test]
fn list_signature_has_provider_flag() {
    let command = AgentModelsList::new();
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(sig.name, "agent models list", "Signature name should match");

    let provider_flag = sig
        .named
        .iter()
        .find(|f| f.long == "provider")
        .expect("Should have --provider flag");

    match provider_flag.arg {
        Some(SyntaxShape::String) => (),
        _ => panic!("--provider flag should accept String argument"),
    }
}
