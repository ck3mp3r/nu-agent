use super::*;
use serial_test::serial;

#[test]
fn generate_config_content_has_header() {
    let content = generate_config_content();
    assert!(content.contains("# nu-agent configuration"));
    assert!(content.contains("agent config init"));
}

#[test]
fn generate_config_content_has_templates() {
    let content = generate_config_content();
    assert!(
        content.contains("store:openai"),
        "should have OpenAI template"
    );
    assert!(
        content.contains("store:anthropic"),
        "should have Anthropic template"
    );
    assert!(
        content.contains("ollama-cloud"),
        "should have Ollama Cloud template"
    );
    assert!(
        content.contains("github-copilot"),
        "should have Copilot template"
    );
}

#[test]
fn generate_config_content_has_no_raw_api_keys() {
    let content = generate_config_content();
    assert!(!content.contains("sk-"), "should not have raw API keys");
}

#[test]
#[serial]
fn generate_config_content_with_env_vars_has_active_model() {
    unsafe {
        std::env::set_var("AGENT_PROVIDER", "openai");
        std::env::set_var("AGENT_MODEL", "gpt-4o");
    }
    let content = generate_config_content();
    assert!(content.contains("[models.default]"));
    assert!(content.contains("model = \"openai/gpt-4o\""));
    unsafe {
        std::env::remove_var("AGENT_PROVIDER");
        std::env::remove_var("AGENT_MODEL");
    }
}

#[test]
fn command_name_is_agent_config_init() {
    let command = AgentConfigInit::new();
    assert_eq!(SimplePluginCommand::name(&command), "agent config init");
}

#[test]
fn command_has_force_flag() {
    let command = AgentConfigInit::new();
    let sig = SimplePluginCommand::signature(&command);
    let force_flag = sig.named.iter().find(|f| f.long == "force");
    assert!(force_flag.is_some(), "Missing --force switch");
}
