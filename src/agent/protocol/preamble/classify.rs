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
        let Some((backend, backend_model)) = model.split_once('/') else {
            return ModelFamily::Unknown;
        };

        if backend == "anthropic" {
            if backend_model.contains("sonnet") {
                return ModelFamily::AnthropicSonnet;
            }
            return ModelFamily::Anthropic;
        }

        if backend == "openai" {
            if backend_model.starts_with("gpt-5") {
                return ModelFamily::Gpt5x;
            }
            return ModelFamily::Gpt4x;
        }
    }

    ModelFamily::Unknown
}
