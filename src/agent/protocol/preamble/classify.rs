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
        return ModelFamily::Unknown;
    }

    if provider == "github-copilot" {
        // Support both legacy format "backend/model" and new format "model"
        if let Some((backend, backend_model)) = model.split_once('/') {
            // Legacy format: use backend for provider hint
            return classify_from_backend_parts(backend, backend_model);
        } else {
            // New format: classify directly from model name
            return classify_by_model_name(&model);
        }
    }

    ModelFamily::Unknown
}

/// Classify model based on backend provider and model name (legacy format)
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
        return ModelFamily::Gpt4x;
    }

    ModelFamily::Unknown
}

/// Classify model based on model name patterns (new format without backend prefix)
fn classify_by_model_name(model: &str) -> ModelFamily {
    // Anthropic models
    if model.contains("claude") || model.contains("sonnet") || model.contains("opus") || model.contains("haiku") {
        if model.contains("sonnet") {
            return ModelFamily::AnthropicSonnet;
        }
        return ModelFamily::Anthropic;
    }

    // OpenAI models
    if model.starts_with("gpt-5") {
        return ModelFamily::Gpt5x;
    }
    if model.starts_with("gpt-4") {
        return ModelFamily::Gpt4x;
    }

    ModelFamily::Unknown
}
