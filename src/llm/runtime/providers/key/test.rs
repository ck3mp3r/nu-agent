use super::ProviderKey;
use crate::config::Config;

fn cfg(provider: &str, model: &str) -> Config {
    Config {
        provider: provider.to_string(),
        provider_impl: None,
        model: model.to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    }
}

#[test]
fn cache_distinguishes_by_model() {
    let k1 = ProviderKey::from_config(&cfg("openai", "gpt-4o"));
    let k2 = ProviderKey::from_config(&cfg("openai", "gpt-4.1"));
    assert_ne!(k1, k2);
}

#[test]
fn cache_distinguishes_by_base_url() {
    let mut c1 = cfg("openai", "gpt-4o");
    c1.base_url = Some("https://a.example".to_string());
    let mut c2 = cfg("openai", "gpt-4o");
    c2.base_url = Some("https://b.example".to_string());

    let k1 = ProviderKey::from_config(&c1);
    let k2 = ProviderKey::from_config(&c2);
    assert_ne!(k1, k2);
}

#[test]
fn cache_distinguishes_by_auth_fingerprint() {
    let mut c1 = cfg("openai", "gpt-4o");
    c1.api_key = Some("token-a".to_string());
    let mut c2 = cfg("openai", "gpt-4o");
    c2.api_key = Some("token-b".to_string());

    let k1 = ProviderKey::from_config(&c1);
    let k2 = ProviderKey::from_config(&c2);

    assert_ne!(k1.auth_fingerprint, k2.auth_fingerprint);
    let dbg = format!("{:?}", k1);
    assert!(!dbg.contains("token-a"));
}

#[test]
fn cache_distinguishes_by_copilot_model_family() {
    let k1 = ProviderKey::from_config(&cfg("github-copilot", "openai/gpt-4o"));
    let k2 = ProviderKey::from_config(&cfg("github-copilot", "openai/gpt-5"));
    let k3 = ProviderKey::from_config(&cfg("github-copilot", "anthropic/claude-sonnet-4.5"));

    assert_ne!(k1.model_family, k2.model_family);
    assert_ne!(k2.model_family, k3.model_family);
}
