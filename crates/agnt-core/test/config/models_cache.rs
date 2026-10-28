use std::collections::HashMap;

use super::{ModelCost, ModelLimit, ModelSpec, ModelsCache, ModelsCacheError, ProviderSpec};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A cache row with no cost, for the listing tests.
fn model_spec(model_id: &str) -> ModelSpec {
    ModelSpec {
        id: model_id.to_string(),
        name: model_id.to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 128_000,
            output: 4_096,
        },
        cost: None,
        modalities: None,
    }
}

fn make_test_cache() -> ModelsCache {
    ModelsCache {
        providers: HashMap::from([(
            "openai".to_string(),
            ProviderSpec {
                id: "openai".to_string(),
                name: "OpenAI".to_string(),
                env: vec!["OPENAI_API_KEY".to_string()],
                api: None,
                models: HashMap::from([(
                    "gpt-4".to_string(),
                    ModelSpec {
                        id: "gpt-4".to_string(),
                        name: "GPT-4".to_string(),
                        tool_call: true,
                        limit: ModelLimit {
                            context: 128000,
                            output: 4096,
                        },
                        cost: None,
                        modalities: None,
                    },
                )]),
            },
        )]),
    }
}

#[test]
fn load_returns_error_when_file_missing() -> Result<()> {
    // Point the loader at a temp dir with no models.json, then verify NotFound.
    let dir = tempfile::tempdir().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("nu-agent").join("models.json");

    let result = ModelsCache::load_from(&path);

    match result {
        Err(ModelsCacheError::NotFound(_)) => (),
        _ => panic!("expected NotFound error, got: {result:?}"),
    }
    Ok(())
}

#[test]
fn get_spec_returns_model_when_found() -> Result<()> {
    let cache = make_test_cache();
    let spec = cache
        .get_spec("openai", "gpt-4")
        .ok_or("spec should be found")?;
    assert_eq!(spec.name, "GPT-4");
    Ok(())
}

#[test]
fn get_spec_returns_none_when_provider_not_found() {
    let cache = make_test_cache();
    let spec = cache.get_spec("anthropic", "claude");
    assert!(spec.is_none());
}

#[test]
fn get_spec_returns_none_when_model_not_found() {
    let cache = make_test_cache();
    let spec = cache.get_spec("openai", "gpt-5");
    assert!(spec.is_none());
}

#[test]
fn list_models_returns_all_without_filter() {
    let cache = make_test_cache();
    let result = cache.list_models(None);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "openai");
    assert_eq!(result[0].1, "gpt-4");
}

#[test]
fn list_models_filters_by_provider() {
    let cache = make_test_cache();
    let result = cache.list_models(Some("openai"));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].1, "gpt-4");

    let empty = cache.list_models(Some("anthropic"));
    assert!(empty.is_empty());
}

#[test]
fn list_models_filters_by_an_exact_cache_key() -> Result<()> {
    // -- Setup & Fixtures: the cache files copilot's models under
    // `github-copilot`, as models.dev does.
    let cache = ModelsCache {
        providers: HashMap::from([(
            "github-copilot".to_string(),
            ProviderSpec {
                id: "github-copilot".to_string(),
                name: "GitHub Copilot".to_string(),
                env: Vec::new(),
                api: None,
                models: HashMap::from([
                    (
                        "claude-sonnet-4.6".to_string(),
                        model_spec("claude-sonnet-4.6"),
                    ),
                    ("gpt-4o".to_string(), model_spec("gpt-4o")),
                ]),
            },
        )]),
    };

    // -- Exec
    let result = cache.list_models(Some("github-copilot"));

    // -- Check
    let mut models: Vec<&str> = result.iter().map(|(_, model, _)| *model).collect();
    models.sort_unstable();
    assert_eq!(
        models,
        vec!["claude-sonnet-4.6", "gpt-4o"],
        "the exact cache key must list its section"
    );
    assert!(
        result
            .iter()
            .all(|(provider, _, _)| *provider == "github-copilot"),
        "the listed rows must carry the key the caller asked for"
    );
    Ok(())
}

#[test]
fn list_models_filters_by_an_undeclared_name_is_empty() -> Result<()> {
    // -- Setup & Fixtures: the cache files copilot's models under
    // `github-copilot`; no name mapping exists.
    let cache = ModelsCache {
        providers: HashMap::from([(
            "github-copilot".to_string(),
            ProviderSpec {
                id: "github-copilot".to_string(),
                name: "GitHub Copilot".to_string(),
                env: Vec::new(),
                api: None,
                models: HashMap::from([("gpt-4o".to_string(), model_spec("gpt-4o"))]),
            },
        )]),
    };

    // -- Exec & Check
    assert!(
        cache.list_models(Some("copilot")).is_empty(),
        "an undeclared name must list nothing: the cache has no such key"
    );
    Ok(())
}

#[test]
fn list_models_filters_by_a_pass_through_provider() -> Result<()> {
    // -- Setup & Fixtures: a name that is already its own cache key.
    let cache = make_test_cache();

    // -- Exec
    let result = cache.list_models(Some("openai"));

    // -- Check
    assert_eq!(result.len(), 1, "a pass-through name must be unchanged");
    assert_eq!(result[0].0, "openai");
    assert_eq!(result[0].1, "gpt-4");
    Ok(())
}

#[test]
fn list_models_filters_by_an_unknown_provider_is_empty() {
    // -- Setup & Fixtures
    let cache = make_test_cache();

    // -- Exec & Check
    assert!(cache.list_models(Some("no-such-provider")).is_empty());
}

#[test]
fn test_models_cache_parse_cost_with_cache_fields() -> Result<()> {
    // -- Setup & Fixtures
    let json = r#"{
        "openai": {
            "id": "openai",
            "name": "OpenAI",
            "models": {
                "gpt-4": {
                    "id": "gpt-4",
                    "name": "GPT-4",
                    "limit": { "context": 128000, "output": 4096 },
                    "cost": {
                        "input": 1.0,
                        "output": 2.7,
                        "cache_read": 0.05,
                        "cache_write": 1.25
                    }
                }
            }
        }
    }"#;

    // -- Exec
    let cache: ModelsCache = serde_json::from_str(json)?;

    // -- Check
    let cost = cache
        .get_spec("openai", "gpt-4")
        .ok_or("spec should be found")?
        .cost
        .as_ref()
        .ok_or("cost should be present")?;
    assert_eq!(cost.input, 1.0);
    assert_eq!(cost.output, 2.7);
    assert_eq!(cost.cache_read, Some(0.05));
    assert_eq!(cost.cache_write, Some(1.25));
    Ok(())
}

#[test]
fn test_models_cache_load_from_cost_without_cache_fields() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("models.json");
    let json = r#"{
        "openai": {
            "id": "openai",
            "name": "OpenAI",
            "models": {
                "gpt-4": {
                    "id": "gpt-4",
                    "name": "GPT-4",
                    "limit": { "context": 128000, "output": 4096 },
                    "cost": { "input": 1.0, "output": 2.7 }
                }
            }
        }
    }"#;
    std::fs::write(&path, json)?;

    // -- Exec
    let cache = ModelsCache::load_from(&path)?;

    // -- Check
    let cost = cache
        .get_spec("openai", "gpt-4")
        .ok_or("spec should be found")?
        .cost
        .as_ref()
        .ok_or("cost should be present")?;
    assert_eq!(cost.cache_read, None);
    assert_eq!(cost.cache_write, None);
    Ok(())
}

#[test]
fn test_models_cache_cost_round_trip_preserves_cache_fields() -> Result<()> {
    // -- Setup & Fixtures
    let cost = ModelCost {
        input: 1.0,
        output: 2.7,
        cache_read: Some(0.05),
        cache_write: Some(1.25),
    };

    // -- Exec
    let json = serde_json::to_string(&cost)?;
    let restored: ModelCost = serde_json::from_str(&json)?;

    // -- Check
    assert_eq!(restored, cost);
    Ok(())
}
