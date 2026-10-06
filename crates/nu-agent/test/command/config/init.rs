use super::*;
use nu_agent_core::utils::env_map::EnvMap;

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
fn generate_config_content_with_env_vars_has_active_model() {
    let env = EnvMap::from([
        ("AGENT_PROVIDER".to_string(), "openai".to_string()),
        ("AGENT_MODEL".to_string(), "gpt-4o".to_string()),
    ]);
    let content = generate_config_content_with(&env);
    assert!(content.contains("[models.default]"));
    assert!(content.contains("model = \"openai/gpt-4o\""));
}

#[test]
fn command_name_is_agent_config_init() {
    let command = AgentConfigInit;
    assert_eq!(SimplePluginCommand::name(&command), "agent config init");
}

#[test]
fn command_has_force_flag() {
    let command = AgentConfigInit;
    let sig = SimplePluginCommand::signature(&command);
    let force_flag = sig.named.iter().find(|f| f.long == "force");
    assert!(force_flag.is_some(), "Missing --force switch");
}
