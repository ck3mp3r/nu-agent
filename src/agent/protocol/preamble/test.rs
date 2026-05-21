use super::{
    ModelFamily, PreambleDefaults, UserPreambleInput, classify_model_family, resolve_preamble,
};

const ASSET_GITHUB_COPILOT_OPENAI_GPT5X: &str =
    include_str!("defaults/github_copilot_openai_gpt5x.md");
const ASSET_GITHUB_COPILOT_OPENAI_GPT4X: &str =
    include_str!("defaults/github_copilot_openai_gpt4x.md");
const ASSET_GITHUB_COPILOT_ANTHROPIC: &str = include_str!("defaults/github_copilot_anthropic.md");
const ASSET_GITHUB_COPILOT_ANTHROPIC_SONNET: &str =
    include_str!("defaults/github_copilot_anthropic_sonnet.md");
const ASSET_OPENAI_GPT5X: &str = include_str!("defaults/openai_gpt5x.md");
const ASSET_OPENAI_GPT4X: &str = include_str!("defaults/openai_gpt4x.md");
const ASSET_ANTHROPIC: &str = include_str!("defaults/anthropic.md");
const ASSET_GLOBAL_FALLBACK: &str = include_str!("defaults/global_fallback.md");

fn base_defaults() -> PreambleDefaults {
    let mut defaults = PreambleDefaults::builtin();
    defaults.set_provider_preamble("openai", "builtin_provider_openai");
    defaults.set_provider_preamble("anthropic", "builtin_provider_anthropic");
    defaults.set_provider_preamble("github-copilot", "builtin_provider_copilot");
    defaults.set_provider_family_preamble("openai", ModelFamily::Gpt5x, "builtin_pf_openai_gpt5x");
    defaults.set_provider_family_preamble("openai", ModelFamily::Gpt4x, "builtin_pf_openai_gpt4x");
    defaults.set_provider_family_preamble(
        "github-copilot",
        ModelFamily::Gpt5x,
        "builtin_pf_copilot_openai_gpt5x",
    );
    defaults.set_provider_family_preamble(
        "github-copilot",
        ModelFamily::Gpt4x,
        "builtin_pf_copilot_openai_gpt4x",
    );
    defaults.set_provider_family_preamble(
        "github-copilot",
        ModelFamily::Anthropic,
        "builtin_pf_copilot_anthropic",
    );
    defaults.set_provider_family_preamble(
        "github-copilot",
        ModelFamily::AnthropicSonnet,
        "builtin_pf_copilot_anthropic_sonnet",
    );
    defaults.set_global_fallback(Some("builtin_global_fallback".to_string()));
    defaults
}

fn mk_input(
    provider: &str,
    family: ModelFamily,
    user_provider: Option<&str>,
    user_provider_family: Option<&str>,
) -> UserPreambleInput {
    UserPreambleInput {
        provider: provider.to_string(),
        model_family: Some(family),
        user_provider_preamble: user_provider.map(|s| s.to_string()),
        user_provider_family_preamble: user_provider_family.map(|s| s.to_string()),
    }
}

#[test]
fn classify_model_family_openai_and_anthropic() {
    assert_eq!(
        classify_model_family("openai", "gpt-5-preview"),
        ModelFamily::Gpt5x
    );
    assert_eq!(
        classify_model_family("openai", "gpt-4o"),
        ModelFamily::Gpt4x
    );
    assert_eq!(
        classify_model_family("anthropic", "claude-sonnet-4.5"),
        ModelFamily::AnthropicSonnet
    );
}

#[test]
fn classify_model_family_github_copilot_nested_backend_models() {
    assert_eq!(
        classify_model_family("github-copilot", "openai/gpt-5-mini"),
        ModelFamily::Gpt5x
    );
    assert_eq!(
        classify_model_family("github-copilot", "openai/gpt-4o"),
        ModelFamily::Gpt4x
    );
    assert_eq!(
        classify_model_family("github-copilot", "openai/o3-mini"),
        ModelFamily::Gpt4x
    );
    assert_eq!(
        classify_model_family("github-copilot", "anthropic/claude-sonnet-4.5"),
        ModelFamily::AnthropicSonnet
    );
}

#[test]
fn classify_model_family_anthropic_sonnet_vs_opus() {
    // Sonnet models should be classified as AnthropicSonnet
    assert_eq!(
        classify_model_family("anthropic", "claude-sonnet-4-20250514"),
        ModelFamily::AnthropicSonnet
    );
    assert_eq!(
        classify_model_family("anthropic", "claude-3-5-sonnet-20241022"),
        ModelFamily::AnthropicSonnet
    );
    assert_eq!(
        classify_model_family("anthropic", "CLAUDE-SONNET-4.5"),
        ModelFamily::AnthropicSonnet
    );

    // Opus and other models should be classified as Anthropic
    assert_eq!(
        classify_model_family("anthropic", "claude-opus-4-20250514"),
        ModelFamily::Anthropic
    );
    assert_eq!(
        classify_model_family("anthropic", "claude-3-opus-20240229"),
        ModelFamily::Anthropic
    );
    assert_eq!(
        classify_model_family("anthropic", "claude-haiku-4"),
        ModelFamily::Anthropic
    );
}

#[test]
fn classify_model_family_github_copilot_anthropic_sonnet_vs_opus() {
    // Sonnet models via GitHub Copilot should be AnthropicSonnet
    assert_eq!(
        classify_model_family("github-copilot", "anthropic/claude-sonnet-4-20250514"),
        ModelFamily::AnthropicSonnet
    );
    assert_eq!(
        classify_model_family("github-copilot", "anthropic/claude-3-5-sonnet-20241022"),
        ModelFamily::AnthropicSonnet
    );

    // Opus models via GitHub Copilot should be Anthropic
    assert_eq!(
        classify_model_family("github-copilot", "anthropic/claude-opus-4-20250514"),
        ModelFamily::Anthropic
    );
    assert_eq!(
        classify_model_family("github-copilot", "anthropic/claude-3-opus-20240229"),
        ModelFamily::Anthropic
    );
}

#[test]
fn classify_model_family_unknown_cases() {
    assert_eq!(
        classify_model_family("openai", "omni-3"),
        ModelFamily::Unknown
    );
    assert_eq!(
        classify_model_family("github-copilot", "openai"),
        ModelFamily::Unknown
    );
    assert_eq!(
        classify_model_family("unknown", "x/y"),
        ModelFamily::Unknown
    );
}

#[test]
fn resolve_preamble_uses_user_provider_family_first() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        mk_input(
            "openai",
            ModelFamily::Gpt5x,
            Some("user_provider"),
            Some("user_provider_family"),
        ),
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("user_provider_family"));
}

#[test]
fn resolve_preamble_falls_back_to_user_provider() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        mk_input("openai", ModelFamily::Gpt5x, Some("user_provider"), None),
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("user_provider"));
}

#[test]
fn resolve_preamble_falls_back_to_builtin_provider_family() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        mk_input("openai", ModelFamily::Gpt5x, None, None),
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("builtin_pf_openai_gpt5x"));
}

#[test]
fn resolve_preamble_falls_back_to_builtin_provider() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        UserPreambleInput {
            provider: "anthropic".to_string(),
            model_family: Some(ModelFamily::Unknown),
            user_provider_preamble: None,
            user_provider_family_preamble: None,
        },
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("builtin_provider_anthropic"));
}

#[test]
fn resolve_preamble_falls_back_to_global_default_on_complete_miss() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        UserPreambleInput {
            provider: "totally-unknown".to_string(),
            model_family: Some(ModelFamily::Unknown),
            user_provider_preamble: None,
            user_provider_family_preamble: None,
        },
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("builtin_global_fallback"));
}

#[test]
fn resolve_preamble_ignores_unknown_family_and_uses_provider() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        UserPreambleInput {
            provider: "openai".to_string(),
            model_family: Some(ModelFamily::Unknown),
            user_provider_preamble: None,
            user_provider_family_preamble: None,
        },
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("builtin_provider_openai"));
}

#[test]
fn resolve_preamble_github_copilot_anthropic_sonnet() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        mk_input("github-copilot", ModelFamily::AnthropicSonnet, None, None),
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("builtin_pf_copilot_anthropic_sonnet"));
}

#[test]
fn resolve_preamble_github_copilot_anthropic_opus() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        mk_input("github-copilot", ModelFamily::Anthropic, None, None),
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("builtin_pf_copilot_anthropic"));
}

#[test]
fn resolve_preamble_trims_and_normalizes_user_values() {
    let defaults = base_defaults();
    let result = resolve_preamble(
        mk_input(
            "openai",
            ModelFamily::Gpt5x,
            Some("  user_provider_trimmed  "),
            Some("   \n\t   "),
        ),
        &defaults,
    );
    assert_eq!(result.as_deref(), Some("user_provider_trimmed"));
}

#[test]
fn resolve_preamble_catalog_includes_required_builtin_targets() {
    let defaults = PreambleDefaults::builtin();

    assert_eq!(
        defaults.provider_family_preamble("github-copilot", ModelFamily::Gpt5x),
        Some(ASSET_GITHUB_COPILOT_OPENAI_GPT5X.trim())
    );
    assert_eq!(
        defaults.provider_family_preamble("github-copilot", ModelFamily::Gpt4x),
        Some(ASSET_GITHUB_COPILOT_OPENAI_GPT4X.trim())
    );
    assert_eq!(
        defaults.provider_family_preamble("github-copilot", ModelFamily::Anthropic),
        Some(ASSET_GITHUB_COPILOT_ANTHROPIC.trim())
    );
    assert_eq!(
        defaults.provider_family_preamble("github-copilot", ModelFamily::AnthropicSonnet),
        Some(ASSET_GITHUB_COPILOT_ANTHROPIC_SONNET.trim())
    );
    assert_eq!(
        defaults.provider_family_preamble("openai", ModelFamily::Gpt5x),
        Some(ASSET_OPENAI_GPT5X.trim())
    );
    assert_eq!(
        defaults.provider_family_preamble("openai", ModelFamily::Gpt4x),
        Some(ASSET_OPENAI_GPT4X.trim())
    );
    assert_eq!(
        defaults.provider_preamble("anthropic"),
        Some(ASSET_ANTHROPIC.trim())
    );
    assert_eq!(
        defaults.global_fallback(),
        Some(ASSET_GLOBAL_FALLBACK.trim())
    );
}

#[test]
fn builtin_assets_are_non_empty_after_trim() {
    for asset in [
        ASSET_GITHUB_COPILOT_OPENAI_GPT5X,
        ASSET_GITHUB_COPILOT_OPENAI_GPT4X,
        ASSET_GITHUB_COPILOT_ANTHROPIC,
        ASSET_GITHUB_COPILOT_ANTHROPIC_SONNET,
        ASSET_OPENAI_GPT5X,
        ASSET_OPENAI_GPT4X,
        ASSET_ANTHROPIC,
        ASSET_GLOBAL_FALLBACK,
    ] {
        assert!(
            !asset.trim().is_empty(),
            "asset should not be empty after trim"
        );
    }
}

#[test]
fn builtin_required_slots_are_mapped() {
    let defaults = PreambleDefaults::builtin();

    assert!(
        defaults
            .provider_family_preamble("github-copilot", ModelFamily::Gpt5x)
            .is_some()
    );
    assert!(
        defaults
            .provider_family_preamble("github-copilot", ModelFamily::Gpt4x)
            .is_some()
    );
    assert!(
        defaults
            .provider_family_preamble("github-copilot", ModelFamily::Anthropic)
            .is_some()
    );
    assert!(
        defaults
            .provider_family_preamble("github-copilot", ModelFamily::AnthropicSonnet)
            .is_some()
    );
    assert!(
        defaults
            .provider_family_preamble("openai", ModelFamily::Gpt5x)
            .is_some()
    );
    assert!(
        defaults
            .provider_family_preamble("openai", ModelFamily::Gpt4x)
            .is_some()
    );
    assert!(defaults.provider_preamble("anthropic").is_some());
    assert!(defaults.global_fallback().is_some());
}
