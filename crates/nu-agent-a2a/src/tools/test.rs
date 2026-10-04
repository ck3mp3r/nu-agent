use std::sync::Arc;

use crate::*;
use serde_json::Value;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn tool_def(name: &str) -> Result<A2aToolDef> {
    a2a_tool_defs()
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("should find tool {name}").into())
}

// ---------------------------------------------------------------------------
// Tool definition tests (sync)
// ---------------------------------------------------------------------------

#[test]
fn test_tool_defs_returns_six_tools() {
    let defs = a2a_tool_defs();
    assert_eq!(defs.len(), 6, "Should have 6 tool definitions");
}

#[test]
fn test_agent_list_has_no_parameters() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "agent_list")
        .ok_or("should find agent_list tool")?;
    let props = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        props.is_empty(),
        "agent_list should have no parameters (no filter) — LLM should get all agents"
    );
    Ok(())
}

#[test]
fn test_agent_get_card_requires_name() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "agent_getCard")
        .ok_or("should find agent_getCard tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("name".into())));
    Ok(())
}

#[test]
fn test_tasks_send_requires_target_and_text() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_send")
        .ok_or("should find tasks_send tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("target".into())));
    assert!(required.contains(&Value::String("text".into())));
    Ok(())
}

#[test]
fn test_tasks_send_schema_has_no_legacy_session_field() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_send")
        .ok_or("should find tasks_send tool")?;
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        !properties.contains_key("sessionId"),
        "tasks_send schema must not list sessionId"
    );
    Ok(())
}

#[test]
fn test_tasks_send_has_optional_context_id_param() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_send")
        .ok_or("should find tasks_send tool")?;
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        properties.contains_key("contextId"),
        "tasks_send should have optional contextId param"
    );
    assert_eq!(properties["contextId"]["type"], "string");

    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(
        !required.contains(&Value::String("contextId".into())),
        "contextId must be optional"
    );
    Ok(())
}

#[test]
fn test_tasks_get_requires_task_id_and_target() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_get")
        .ok_or("should find tasks_get tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("taskId".into())));
    assert!(required.contains(&Value::String("target".into())));
    Ok(())
}

#[test]
fn test_tasks_cancel_requires_task_id_and_target() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_cancel")
        .ok_or("should find tasks_cancel tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("taskId".into())));
    assert!(required.contains(&Value::String("target".into())));
    Ok(())
}

#[test]
fn test_tasks_list_has_optional_status_param() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_list")
        .ok_or("should find tasks_list tool")?;
    assert!(
        tool.parameters.get("required").is_none(),
        "tasks.list should have no required params"
    );
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        properties.contains_key("status"),
        "tasks.list should have optional status param"
    );
    assert_eq!(properties["status"]["type"], "string");
    assert!(
        properties.contains_key("target"),
        "tasks.list should have optional target param"
    );

    // Every advertised status value must parse via TaskState::try_from, so the
    // schema and the strict parser cannot drift apart.
    let description = properties["status"]["description"]
        .as_str()
        .ok_or("status should have a description")?;
    let tokens: Vec<&str> = description
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .collect();
    let advertised: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| t.starts_with("TASK_STATE_"))
        .collect();
    assert!(
        !advertised.is_empty(),
        "status description should advertise TASK_STATE_* values, got: {description}"
    );
    for value in advertised {
        assert!(
            TaskState::try_from(value).is_ok(),
            "advertised status value {value} must parse via TaskState::try_from"
        );
    }
    Ok(())
}

#[test]
fn test_all_definitions_have_valid_json_schema() {
    for tool in a2a_tool_defs() {
        assert!(tool.parameters.get("type").is_some(), "missing type");
        assert!(
            tool.parameters.get("properties").is_some(),
            "missing properties"
        );
    }
}

#[test]
fn test_descriptions_avoid_banned_words() -> Result<()> {
    // -- Setup & Fixtures
    let banned = ["appropriate", "should", "etc.", "as needed"];

    // -- Exec & Check
    for tool in a2a_tool_defs() {
        let lower = tool.description.to_lowercase();
        for word in banned {
            assert!(
                !lower.contains(word),
                "{} description must not contain banned word {word:?}",
                tool.name
            );
        }
    }
    Ok(())
}

#[test]
fn test_descriptions_are_within_token_budget() -> Result<()> {
    // -- Setup & Fixtures
    const MAX_CHARS: usize = 500;

    // -- Exec & Check
    for tool in a2a_tool_defs() {
        assert!(
            tool.description.chars().count() <= MAX_CHARS,
            "{} description is {} chars, over the {MAX_CHARS} budget",
            tool.name,
            tool.description.chars().count()
        );
    }
    Ok(())
}

#[test]
fn test_tasks_send_description_documents_async_and_session() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_send")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(d.contains("asynchronously"), "must state async execution");
    assert!(d.contains("taskId"), "must name the taskId return");
    assert!(
        d.contains("conversation turn"),
        "must state completion arrives as a conversation turn"
    );
    assert!(
        d.contains("omit contextId") && d.contains("fresh session"),
        "must explain omitting contextId starts a fresh session"
    );
    assert!(
        d.contains("include contextId") && d.contains("resume"),
        "must explain including contextId resumes a session"
    );
    assert!(
        d.contains("lose all prior context"),
        "must state the consequence of omitting contextId"
    );
    assert!(
        d.contains("ALWAYS") && d.contains("contextId"),
        "must instruct the LLM to always reuse contextId on follow-up tasks"
    );
    assert!(
        d.contains("{taskId, contextId, status, message}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_tasks_get_description_documents_remote_and_shape() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_get")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("remote agent"),
        "must state the fetch is from a remote agent"
    );
    assert!(
        !d.contains("LOCAL"),
        "must not claim the task store is local"
    );
    assert!(
        d.contains("{taskId, state, artifacts}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_tasks_list_description_documents_both_modes() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_list")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("With target") && d.contains("remote agent"),
        "must document the remote mode"
    );
    assert!(
        d.contains("Without target") && d.contains("local store"),
        "must document the local mode"
    );
    for value in [
        "TASK_STATE_UNSPECIFIED",
        "TASK_STATE_SUBMITTED",
        "TASK_STATE_WORKING",
        "TASK_STATE_INPUT_REQUIRED",
        "TASK_STATE_COMPLETED",
        "TASK_STATE_FAILED",
        "TASK_STATE_CANCELED",
        "TASK_STATE_REJECTED",
        "TASK_STATE_AUTH_REQUIRED",
    ] {
        assert!(d.contains(value), "must list status value {value}");
    }
    assert!(
        d.contains("{tasks: [...]}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_tasks_cancel_description_documents_usage_and_shape() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_cancel")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("no longer needed") || d.contains("runs too long"),
        "must explain when to cancel"
    );
    assert!(
        d.contains("taskId") && d.contains("target") && d.contains("required"),
        "must state both taskId and target are required"
    );
    assert!(
        d.contains("{taskId, state}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_agent_list_description_documents_discovery_and_shape() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("agent_list")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("Call this first") && d.contains("discover"),
        "must state this is the discovery step"
    );
    assert!(
        d.contains("Excludes yourself"),
        "must state self is excluded"
    );
    assert!(
        d.contains("{agents: [{name, url, description, skills}]}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_agent_get_card_description_documents_contents_and_when() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("agent_getCard")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("capabilities") && d.contains("skills") && d.contains("security"),
        "must describe what a card contains"
    );
    assert!(
        d.contains("agent_list already gives"),
        "must contrast with agent_list"
    );
    assert!(
        d.contains("{name, error}"),
        "must document the failure shape"
    );
    Ok(())
}

#[test]
fn test_tasks_list_status_schema_lists_all_values() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_list")?;
    let description = tool.parameters["properties"]["status"]["description"]
        .as_str()
        .ok_or("status should have a description")?;

    // -- Exec & Check
    for value in [
        "TASK_STATE_UNSPECIFIED",
        "TASK_STATE_SUBMITTED",
        "TASK_STATE_WORKING",
        "TASK_STATE_INPUT_REQUIRED",
        "TASK_STATE_COMPLETED",
        "TASK_STATE_FAILED",
        "TASK_STATE_CANCELED",
        "TASK_STATE_REJECTED",
        "TASK_STATE_AUTH_REQUIRED",
    ] {
        assert!(
            description.contains(value),
            "status schema must list {value}, got: {description}"
        );
    }
    Ok(())
}

#[test]
fn test_tasks_send_schema_context_id_is_optional_string() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_send")?;
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;

    // -- Exec & Check
    assert_eq!(properties["contextId"]["type"], "string");
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(
        !required.contains(&Value::String("contextId".into())),
        "contextId must be optional"
    );
    Ok(())
}

static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// Handler tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handle_agent_list_empty() -> Result<()> {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let result = handle_agent_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let agents = result["agents"]
        .as_array()
        .ok_or("should have agents array")?;
    assert_eq!(agents.len(), 0);
    Ok(())
}

#[tokio::test]
async fn test_handle_agent_list_with_peers() -> Result<()> {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: Some(AgentCard {
            name: "alice".into(),
            description: Some("Alice agent".into()),
            skills: vec![Skill {
                id: "chat".into(),
                name: "Chat".into(),
                description: "Chatting".into(),
                inputs: None,
                outputs: None,
            }],
            ..Default::default()
        }),
        discovered_at: std::time::Instant::now(),
    });
    let result = handle_agent_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let agents = result["agents"]
        .as_array()
        .ok_or("should have agents array")?;
    assert_eq!(agents.len(), 1);
    assert_eq!(result["agents"][0]["name"], "alice");
    Ok(())
}

#[tokio::test]
async fn test_handle_agent_get_card_not_found() {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let params = serde_json::json!({"name": "nonexistent"});
    let result = handle_agent_get_card(ctx, params).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[tokio::test]
async fn test_handle_tasks_send_missing_param() {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let params = serde_json::json!({"target": "someone"});
    let result = handle_tasks_send(ctx, params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_tasks_send_to_real_server() -> Result<()> {
    ensure_crypto_provider();
    // Start real server, add to cache, send task via handler
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.get("taskId").is_some(), "Should have a taskId");
    assert_eq!(
        result["status"], "sent",
        "tasks.send now returns status 'sent' (fire-and-forget)"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_with_context_id_returns_context_id() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    // -- Exec
    let params = serde_json::json!({
        "target": "test-agent",
        "text": "Hello!",
        "contextId": "ctx-abc"
    });
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        result["contextId"], "ctx-abc",
        "tool result should echo the server-assigned contextId"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_without_context_id_returns_uuid() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    // -- Exec
    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert!(
        result.get("contextId").is_some(),
        "tool result should always carry a contextId key"
    );
    assert!(
        result["contextId"].is_string(),
        "contextId should be a string when not provided, got: {}",
        result["contextId"]
    );
    let context_id = result["contextId"]
        .as_str()
        .ok_or("contextId should be a string")?;
    assert!(
        uuid::Uuid::parse_str(context_id).is_ok(),
        "contextId should be a UUID when not provided, got: {context_id}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_with_own_card_url() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard {
            name: "sender".into(),
            url: "http://sender.local:34567".into(),
            ..Default::default()
        },
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.get("taskId").is_some(), "Should have a taskId");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_get_to_real_server() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    // First send a task
    let send_params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let send_result = handle_tasks_send(ctx.clone(), send_params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let task_id = send_result["taskId"]
        .as_str()
        .ok_or("should have taskId string")?
        .to_string();

    // Then get it
    let get_params = serde_json::json!({"target": "test-agent", "taskId": task_id});
    let get_result = handle_tasks_get(ctx, get_params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(get_result["taskId"], task_id);

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// tasks.list handler
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handle_tasks_list_with_local_store() -> Result<()> {
    ensure_crypto_provider();
    let store = Arc::new(InMemoryTaskStore::default());
    store.create_task(None, None, None);
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 2, "Should list 2 tasks from local store");
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_with_local_store_filtered() -> Result<()> {
    ensure_crypto_provider();
    let store = Arc::new(InMemoryTaskStore::default());
    let t1 = store.create_task(None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({"status": "TASK_STATE_WORKING"}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 1, "Should list 1 working task");
    assert_eq!(tasks[0]["status"]["state"], "TASK_STATE_WORKING");
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_with_local_store_filtered_auth_required() -> Result<()> {
    ensure_crypto_provider();
    let store = Arc::new(InMemoryTaskStore::default());
    let t1 = store.create_task(None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    // No task is in AuthRequired, but the filter must parse and apply — an
    // unparseable status would return Err instead of an empty list.
    let result = handle_tasks_list(
        ctx,
        serde_json::json!({"status": "TASK_STATE_AUTH_REQUIRED"}),
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert!(
        tasks.is_empty(),
        "AuthRequired filter should match no tasks, got {tasks:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_invalid_status_returns_error() -> Result<()> {
    ensure_crypto_provider();
    let store = Arc::new(InMemoryTaskStore::default());
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({"status": "INVALID_BOGUS"})).await;
    let err = result
        .err()
        .ok_or("invalid status should return an error")?;
    assert!(
        err.contains("Invalid status") && err.contains("INVALID_BOGUS"),
        "error should name the invalid status, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_no_status_returns_all() -> Result<()> {
    ensure_crypto_provider();
    let store = Arc::new(InMemoryTaskStore::default());
    let t1 = store.create_task(None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 2, "Should list all tasks when no status given");
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_no_store() -> Result<()> {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert!(tasks.is_empty(), "Should return empty list when no store");
    Ok(())
}

#[tokio::test]
async fn agent_list_excludes_self() -> Result<()> {
    ensure_crypto_provider();
    let own_url = "http://127.0.0.1:9999".to_string();
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard {
            name: "self-agent".into(),
            url: own_url.clone(),
            ..Default::default()
        },
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };

    // Peer with same URL → should be excluded from output
    ctx.cache.add_or_update(Peer {
        name: "self-agent".into(),
        url: own_url.clone(),
        host: "127.0.0.1".into(),
        port: 9999,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    // Peer with different URL → should appear in output
    ctx.cache.add_or_update(Peer {
        name: "other-agent".into(),
        url: "http://127.0.0.1:8888".into(),
        host: "127.0.0.1".into(),
        port: 8888,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    let result = handle_agent_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let agents = result["agents"]
        .as_array()
        .ok_or("should have agents array")?;

    // Self should be excluded — only the other agent remains
    assert_eq!(agents.len(), 1, "self-agent should be excluded from output");

    let other_agent = &agents[0];
    assert_eq!(other_agent["name"], "other-agent");

    // is_self field must NOT be present in any entry
    for agent in agents {
        let obj = agent.as_object().ok_or("should be an object")?;
        assert!(
            !obj.contains_key("is_self"),
            "no entry should have an is_self field"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_completion_event_carries_context_id() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();

    let (completion_tx, mut completion_rx) = tokio::sync::mpsc::channel::<A2aCompletionEvent>(16);
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: Some(completion_tx),
        runtime_handle: Some(tokio::runtime::Handle::current()),
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    // -- Exec
    let params = serde_json::json!({
        "target": "test-agent",
        "text": "Hello!",
        "contextId": "ctx-abc"
    });
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let task_id = result["taskId"]
        .as_str()
        .ok_or("should have taskId")?
        .to_string();

    // Let the background SSE watcher subscribe, then complete the task so the
    // watcher observes a terminal state and emits the completion event.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    server
        .task_store()
        .update_status(&task_id, TaskState::Completed, None)
        .map_err(|e| format!("update_status should succeed: {e:?}"))?;

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), completion_rx.recv())
        .await
        .map_err(|_| "completion event should arrive")?
        .ok_or("completion channel should not close")?;

    // -- Check
    assert_eq!(
        event.context_id.as_deref(),
        Some("ctx-abc"),
        "completion event must carry the task's contextId"
    );

    server.shutdown().await;
    Ok(())
}
