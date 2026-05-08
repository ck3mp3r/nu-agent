use super::{ModelFamily, PreambleDefaults};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPreambleInput {
    pub provider: String,
    pub model_family: Option<ModelFamily>,
    pub user_provider_preamble: Option<String>,
    pub user_provider_family_preamble: Option<String>,
}

fn normalize_preamble(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn resolve_preamble(input: UserPreambleInput, defaults: &PreambleDefaults) -> Option<String> {
    let provider = input.provider.trim().to_lowercase();
    let family = input
        .model_family
        .and_then(|f| if f == ModelFamily::Unknown { None } else { Some(f) });

    normalize_preamble(input.user_provider_family_preamble.as_deref())
        .or_else(|| normalize_preamble(input.user_provider_preamble.as_deref()))
        .or_else(|| {
            family.and_then(|f| defaults.provider_family_preamble(&provider, f).map(str::to_string))
        })
        .or_else(|| defaults.provider_preamble(&provider).map(str::to_string))
        .or_else(|| defaults.global_fallback().map(str::to_string))
}
