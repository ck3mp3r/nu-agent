//! Credential resolution and model erasure for the provider clients.
//!
//! These tests build real provider clients (HTTP transport + TLS), so the
//! module is gated behind the `integration` feature.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

use crate::config::Config;
use crate::utils::env_map::EnvMap;

// ========================================================================
// build_dyn_model: every cached variant erases to a DynModel<Completion>
// ========================================================================

/// Drive one completion through an erased model and return the response text.
async fn drive_erased_model(model: &rig::DynModel<rig::operation::Completion>) -> Result<String> {
    use futures::StreamExt;
    let stream = model
        .stream(rig::completion::CompletionRequest::new("hello"))
        .map_err(|e| format!("completion stream: {e}"))?;
    let mut stream = std::pin::pin!(stream);
    let mut text = String::new();
    while let Some(item) = stream.next().await {
        let item = item.map_err(|e| format!("stream item: {e}"))?;
        if let rig::streaming::Item::Event(rig::streaming::StreamEvent::Text {
            text: delta, ..
        }) = item
        {
            text.push_str(&delta);
        }
    }
    Ok(text)
}

/// Every `CachedProviderClient` variant must erase to the one shared
/// `DynModel<Completion>` type. The mock variant is driven end-to-end so the
/// erasure is proven to run the same driver as the concrete model.
#[tokio::test]
async fn build_dyn_model_erases_every_cached_variant() -> Result<()> {
    // -- Setup & Fixtures
    let openai_config = Config {
        api_key: Some("sk-fake".to_string()),
        ..Config::default()
    };
    let anthropic_config = Config {
        api_key: Some("sk-ant-fake".to_string()),
        ..Config::default()
    };
    let ollama_config = Config {
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };

    let copilot = build_copilot_client_on(
        &Config {
            api_key: Some("tid=static-copilot-key".to_string()),
            ..Config::default()
        },
        &EnvMap::new(),
        None,
        rig::test_utils::RecordingHttpClient::new(""),
    )
    .await?;

    let cases: Vec<(&str, CachedProviderClient)> = vec![
        ("copilot", CachedProviderClient::Copilot(copilot)),
        (
            "openai",
            CachedProviderClient::OpenAi(build_openai_client(&openai_config)?),
        ),
        (
            "openai-completions",
            CachedProviderClient::OpenAiCompletions(build_openai_client(&openai_config)?),
        ),
        (
            "anthropic",
            CachedProviderClient::Anthropic(build_anthropic_client(&anthropic_config)?),
        ),
        (
            "ollama",
            CachedProviderClient::Ollama(build_ollama_client(&ollama_config)?),
        ),
        (
            "mock",
            CachedProviderClient::Mock(rig::test_utils::MockCompletionModel::from_stream_turns([
                [
                    rig::test_utils::MockStreamEvent::Text("mock reply".to_string()),
                    rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
                ],
            ])),
        ),
    ];

    // -- Exec & Check
    for (name, client) in cases {
        let model = client
            .build_dyn_model("test-model")
            .map_err(|e| format!("case '{name}': build_dyn_model must succeed: {e:?}"))?;
        if name == "mock" {
            let text = drive_erased_model(&model).await?;
            assert_eq!(
                text, "mock reply",
                "case '{name}': the erased mock model must run the mock driver"
            );
        }
    }
    Ok(())
}

// ========================================================================
// Copilot auth failure mapping
// ========================================================================

/// A GitHub access-token exchange that answers with a non-JSON body fails with
/// an `AuthError` that is not a missing credential, so the error must be
/// reported as an authentication failure — not as a sign-in instruction.
#[tokio::test]
async fn copilot_non_credential_auth_failure_is_reported_as_authentication_error() -> Result<()> {
    // -- Setup & Fixtures
    let token_dir = tempfile::TempDir::new()?;
    let http = rig::test_utils::RecordingHttpClient::new("not-json");
    let env = EnvMap::from([(
        "COPILOT_GITHUB_ACCESS_TOKEN".to_string(),
        "gho_raw-github-token".to_string(),
    )]);

    // -- Exec
    let error = build_copilot_client_on(
        &Config {
            provider: "copilot".to_string(),
            model: "gpt-4o".to_string(),
            ..Config::default()
        },
        &env,
        Some(token_dir.path().to_path_buf()),
        http.clone(),
    )
    .await
    .err()
    .ok_or("a malformed exchange reply must not build a client")?;

    // -- Check
    let message = error.to_string();
    assert!(
        message.starts_with("GitHub Copilot authentication failed:"),
        "a non-credential failure must be reported as an authentication error: {message}"
    );
    assert!(
        !message.contains("agent provider auth login github-copilot"),
        "a non-credential failure must not instruct the user to sign in: {message}"
    );
    assert_eq!(
        http.requests().len(),
        1,
        "the exchange request must have been attempted"
    );
    Ok(())
}

// ========================================================================
// Environment-variable credential fallbacks
// ========================================================================

/// Run `body` with `key` set to `value`, restoring the previous value after.
fn with_env_var<T>(key: &str, value: Option<&str>, body: impl FnOnce() -> T) -> T {
    let previous = std::env::var_os(key);
    match value {
        Some(value) => unsafe { std::env::set_var(key, value) },
        None => unsafe { std::env::remove_var(key) },
    }
    let result = body();
    match previous {
        Some(previous) => unsafe { std::env::set_var(key, previous) },
        None => unsafe { std::env::remove_var(key) },
    }
    result
}

/// Without a configured `api_key`, the OpenAI client reads `OPENAI_API_KEY`
/// from the environment.
#[test]
#[serial_test::serial]
fn build_openai_client_falls_back_to_openai_api_key_env() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: None,
        ..Config::default()
    };

    // -- Exec
    let client = with_env_var("OPENAI_API_KEY", Some("sk-from-env"), || {
        build_openai_client(&config)
    })
    .map_err(|e| format!("env fallback must build a client: {e:?}"))?;

    // -- Check
    assert_eq!(client.config().api_key.expose(), "sk-from-env");
    Ok(())
}

/// Without a configured `api_key` and without `OPENAI_API_KEY`, the OpenAI
/// client reports the missing credential instead of building a client.
#[test]
#[serial_test::serial]
fn build_openai_client_without_any_key_reports_missing_env() {
    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: None,
        ..Config::default()
    };

    // -- Exec
    let error = with_env_var("OPENAI_API_KEY", None, || build_openai_client(&config))
        .err()
        .map(|e| e.to_string());

    // -- Check
    assert_eq!(
        error.as_deref(),
        Some("OpenAI client initialization failed: OPENAI_API_KEY not set.")
    );
}

/// Without a configured `api_key`, the Anthropic client reads
/// `ANTHROPIC_API_KEY` from the environment.
#[test]
#[serial_test::serial]
fn build_anthropic_client_falls_back_to_anthropic_api_key_env() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: None,
        ..Config::default()
    };

    // -- Exec
    let client = with_env_var("ANTHROPIC_API_KEY", Some("sk-ant-from-env"), || {
        build_anthropic_client(&config)
    })
    .map_err(|e| format!("env fallback must build a client: {e:?}"))?;

    // -- Check
    assert_eq!(client.config().api_key.expose(), "sk-ant-from-env");
    Ok(())
}

/// Without a configured `api_key` and without `ANTHROPIC_API_KEY`, the
/// Anthropic client reports the missing credential.
#[test]
#[serial_test::serial]
fn build_anthropic_client_without_any_key_reports_missing_env() {
    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: None,
        ..Config::default()
    };

    // -- Exec
    let error = with_env_var("ANTHROPIC_API_KEY", None, || {
        build_anthropic_client(&config)
    })
    .err()
    .map(|e| e.to_string());

    // -- Check
    assert_eq!(
        error.as_deref(),
        Some("Anthropic client initialization failed: ANTHROPIC_API_KEY not set.")
    );
}

/// Without a configured `base_url`, the Ollama client reads
/// `OLLAMA_API_BASE_URL` from the environment.
#[test]
#[serial_test::serial]
fn build_ollama_client_falls_back_to_ollama_api_base_url_env() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: None,
        ..Config::default()
    };

    // -- Exec
    let client = with_env_var("OLLAMA_API_BASE_URL", Some("http://env-host:11434"), || {
        build_ollama_client(&config)
    })
    .map_err(|e| format!("env fallback must build a client: {e:?}"))?;

    // -- Check
    assert_eq!(client.config().base_url, "http://env-host:11434");
    Ok(())
}

/// Without a configured `base_url` and without `OLLAMA_API_BASE_URL`, the
/// Ollama client falls back to the local default endpoint.
#[test]
#[serial_test::serial]
fn build_ollama_client_without_base_url_uses_local_default() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: None,
        ..Config::default()
    };

    // -- Exec
    let client = with_env_var("OLLAMA_API_BASE_URL", None, || build_ollama_client(&config))
        .map_err(|e| format!("default base url must build a client: {e:?}"))?;

    // -- Check
    assert_eq!(client.config().base_url, "http://localhost:11434");
    Ok(())
}
