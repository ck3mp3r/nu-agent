use std::collections::HashMap;

use serial_test::serial;

use super::{ModelLimit, ModelSpec, ModelsCache, ModelsCacheError, ProviderSpec};

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
#[serial]
fn load_returns_error_when_file_missing() {
    // Point XDG_DATA_HOME at a temp dir with no models.json, then verify NotFound.
    let dir = tempfile::tempdir().expect("temp dir");
    unsafe {
        std::env::set_var("XDG_DATA_HOME", dir.path());
    }

    let result = ModelsCache::load();

    // Restore env to avoid leaking into other tests.
    unsafe {
        std::env::remove_var("XDG_DATA_HOME");
    }

    match result {
        Err(ModelsCacheError::NotFound(_)) => (),
        _ => panic!("expected NotFound error, got: {result:?}"),
    }
}

#[test]
fn get_spec_returns_model_when_found() {
    let cache = make_test_cache();
    let spec = cache.get_spec("openai", "gpt-4");
    assert!(spec.is_some());
    assert_eq!(spec.unwrap().name, "GPT-4");
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
