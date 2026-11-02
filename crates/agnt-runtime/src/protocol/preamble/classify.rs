//! Model-family classification.
//!
//! Adapter note: a model identity is an external string (`provider/model`), so
//! this module is the one place where identity strings decide behavior. The
//! heuristics live here and nowhere else. An explicit family declaration in
//! model config will supersede them (pluggable model versions); until then this
//! adapter is the only decision point.

use super::ModelFamily;

pub fn classify_model_family(provider: &str, model: &str) -> ModelFamily {
    let provider = provider.trim().to_lowercase();
    let model = model.trim().to_lowercase();

    if provider == "anthropic" {
        if model.contains("sonnet") {
            return ModelFamily::AnthropicSonnet;
        }
        return ModelFamily::Anthropic;
    }

    if provider == "openai" {
        if model.starts_with("gpt-5") {
            return ModelFamily::Gpt5x;
        }
        if model.starts_with("gpt-4") {
            return ModelFamily::Gpt4x;
        }
        // OpenAI reasoning models (o3, o4) are mapped to Gpt4x for now.
        // These models have distinct capabilities (reasoning tokens, limited system prompt)
        // but we don't have a separate ModelFamily::Reasoning variant yet.
        // This is an explicit mapping, not a silent fallthrough.
        if model.starts_with("o3") || model.starts_with("o4") {
            return ModelFamily::Gpt4x;
        }
        return ModelFamily::Unknown;
    }

    if provider == "github-copilot" {
        // A three-part identity (`provider/backend/model`) leaves the backend
        // in the model part: classify from the backend segments.
        if let Some((backend, backend_model)) = model.split_once('/') {
            return classify_from_backend_parts(backend, backend_model);
        }
    }

    ModelFamily::Unknown
}

/// Classify a three-part identity's model part by its backend provider and
/// model name.
///
/// Known divergence, kept as-is until the explicit family declaration lands:
/// the provider path returns `Unknown` for unrecognized OpenAI models, while
/// this backend path treats every non-`gpt-5` OpenAI model as `Gpt4x`. Both
/// fallbacks are recorded here so the difference is a decision, not an
/// accident.
fn classify_from_backend_parts(backend: &str, model: &str) -> ModelFamily {
    if backend == "anthropic" {
        if model.contains("sonnet") {
            return ModelFamily::AnthropicSonnet;
        }
        return ModelFamily::Anthropic;
    }

    if backend == "openai" {
        if model.starts_with("gpt-5") {
            return ModelFamily::Gpt5x;
        }
        // OpenAI reasoning models (o3, o4) are explicitly mapped to Gpt4x.
        // This prevents silent misclassification while we don't have a dedicated variant.
        if model.starts_with("o3") || model.starts_with("o4") {
            return ModelFamily::Gpt4x;
        }
        // Default for other OpenAI models (gpt-4*, gpt-3.5*, etc.)
        return ModelFamily::Gpt4x;
    }

    ModelFamily::Unknown
}
