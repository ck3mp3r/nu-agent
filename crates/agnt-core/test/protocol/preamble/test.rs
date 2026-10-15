use super::{
    ModelFamily, PreambleDefaults, UserPreambleInput, classify_model_family, resolve_preamble,
};

const ASSET_GITHUB_COPILOT_OPENAI_GPT5X: &str =
    include_str!("../../../src/protocol/preamble/defaults/github_copilot_openai_gpt5x.md");
const ASSET_GITHUB_COPILOT_OPENAI_GPT4X: &str =
    include_str!("../../../src/protocol/preamble/defaults/github_copilot_openai_gpt4x.md");
const ASSET_GITHUB_COPILOT_ANTHROPIC: &str =
    include_str!("../../../src/protocol/preamble/defaults/github_copilot_anthropic.md");
const ASSET_GITHUB_COPILOT_ANTHROPIC_SONNET: &str =
    include_str!("../../../src/protocol/preamble/defaults/github_copilot_anthropic_sonnet.md");
const ASSET_OPENAI_GPT5X: &str =
    include_str!("../../../src/protocol/preamble/defaults/openai_gpt5x.md");
const ASSET_OPENAI_GPT4X: &str =
    include_str!("../../../src/protocol/preamble/defaults/openai_gpt4x.md");
const ASSET_ANTHROPIC: &str = include_str!("../../../src/protocol/preamble/defaults/anthropic.md");
const ASSET_GLOBAL_FALLBACK: &str =
    include_str!("../../../src/protocol/preamble/defaults/global_fallback.md");

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
fn classify_model_family_table() {
    let cases: &[(&str, &str, ModelFamily)] = &[
        // OpenAI and Anthropic direct models.
        ("openai", "gpt-5-preview", ModelFamily::Gpt5x),
        ("openai", "gpt-4o", ModelFamily::Gpt4x),
        (
            "anthropic",
            "claude-sonnet-4-20250514",
            ModelFamily::AnthropicSonnet,
        ),
        // GitHub Copilot nested backend models.
        ("github-copilot", "openai/gpt-5-mini", ModelFamily::Gpt5x),
        ("github-copilot", "openai/gpt-4o", ModelFamily::Gpt4x),
        ("github-copilot", "openai/o3-mini", ModelFamily::Gpt4x),
        (
            "github-copilot",
            "anthropic/claude-sonnet-4-20250514",
            ModelFamily::AnthropicSonnet,
        ),
        // Anthropic Sonnet vs Opus.
        (
            "anthropic",
            "claude-3-5-sonnet-20241022",
            ModelFamily::AnthropicSonnet,
        ),
        (
            "anthropic",
            "CLAUDE-SONNET-4.5",
            ModelFamily::AnthropicSonnet,
        ),
        (
            "anthropic",
            "claude-opus-4-20250514",
            ModelFamily::Anthropic,
        ),
        (
            "anthropic",
            "claude-3-opus-20240229",
            ModelFamily::Anthropic,
        ),
        ("anthropic", "claude-haiku-4", ModelFamily::Anthropic),
        // GitHub Copilot Anthropic Sonnet vs Opus.
        (
            "github-copilot",
            "anthropic/claude-3-5-sonnet-20241022",
            ModelFamily::AnthropicSonnet,
        ),
        (
            "github-copilot",
            "anthropic/claude-opus-4-20250514",
            ModelFamily::Anthropic,
        ),
        (
            "github-copilot",
            "anthropic/claude-3-opus-20240229",
            ModelFamily::Anthropic,
        ),
        // Unknown cases.
        ("openai", "omni-3", ModelFamily::Unknown),
        ("github-copilot", "openai", ModelFamily::Unknown),
        ("unknown", "x/y", ModelFamily::Unknown),
        // OpenAI reasoning models (o3, o4) map to Gpt4x.
        // NOTE: These are reasoning models with distinct capabilities (reasoning tokens,
        // limited system prompt support), but we map them to Gpt4x for now as we don't
        // have a separate ModelFamily::Reasoning variant yet.
        ("openai", "o3-mini", ModelFamily::Gpt4x),
        ("openai", "o4-mini", ModelFamily::Gpt4x),
        ("openai", "o3", ModelFamily::Gpt4x),
        ("openai", "o4", ModelFamily::Gpt4x),
        // GitHub Copilot legacy reasoning-model format.
        ("github-copilot", "openai/o4-mini", ModelFamily::Gpt4x),
        ("github-copilot", "openai/o3", ModelFamily::Gpt4x),
    ];

    for (provider, model, expected) in cases {
        assert_eq!(
            classify_model_family(provider, model),
            *expected,
            "provider={provider} model={model}"
        );
    }
}

#[test]
fn resolve_preamble_table() {
    let defaults = base_defaults();
    let cases: &[(UserPreambleInput, Option<&str>)] = &[
        // The user provider-family preamble wins.
        (
            mk_input(
                "openai",
                ModelFamily::Gpt5x,
                Some("user_provider"),
                Some("user_provider_family"),
            ),
            Some("user_provider_family"),
        ),
        // Falls back to the user provider preamble.
        (
            mk_input("openai", ModelFamily::Gpt5x, Some("user_provider"), None),
            Some("user_provider"),
        ),
        // Falls back to the builtin provider-family preamble.
        (
            mk_input("openai", ModelFamily::Gpt5x, None, None),
            Some("builtin_pf_openai_gpt5x"),
        ),
        // Falls back to the builtin provider preamble.
        (
            UserPreambleInput {
                provider: "anthropic".to_string(),
                model_family: Some(ModelFamily::Unknown),
                user_provider_preamble: None,
                user_provider_family_preamble: None,
            },
            Some("builtin_provider_anthropic"),
        ),
        // Falls back to the global default on a complete miss.
        (
            UserPreambleInput {
                provider: "totally-unknown".to_string(),
                model_family: Some(ModelFamily::Unknown),
                user_provider_preamble: None,
                user_provider_family_preamble: None,
            },
            Some("builtin_global_fallback"),
        ),
        // An unknown family is ignored and the provider preamble is used.
        (
            UserPreambleInput {
                provider: "openai".to_string(),
                model_family: Some(ModelFamily::Unknown),
                user_provider_preamble: None,
                user_provider_family_preamble: None,
            },
            Some("builtin_provider_openai"),
        ),
        // GitHub Copilot Anthropic Sonnet.
        (
            mk_input("github-copilot", ModelFamily::AnthropicSonnet, None, None),
            Some("builtin_pf_copilot_anthropic_sonnet"),
        ),
        // GitHub Copilot Anthropic Opus.
        (
            mk_input("github-copilot", ModelFamily::Anthropic, None, None),
            Some("builtin_pf_copilot_anthropic"),
        ),
        // User values are trimmed and normalized.
        (
            mk_input(
                "openai",
                ModelFamily::Gpt5x,
                Some("  user_provider_trimmed  "),
                Some("   \n\t   "),
            ),
            Some("user_provider_trimmed"),
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(
            resolve_preamble(input.clone(), &defaults).as_deref(),
            *expected,
            "provider={} family={:?}",
            input.provider,
            input.model_family
        );
    }
}

#[test]
fn builtin_preamble_catalog_and_assets() {
    let defaults = PreambleDefaults::builtin();

    // The catalog maps every required builtin target to its asset.
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

    // Every builtin asset is non-empty after trim.
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

    // Every required slot is mapped.
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
