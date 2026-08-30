use std::sync::Arc;

use crate::*;
use serde_json::Value;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
    let server = A2aServer::start(card, Arc::new(PeerCache::default()), 0)
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
async fn test_handle_tasks_send_with_own_card_url() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::default()), 0)
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
    let server = A2aServer::start(card, Arc::new(PeerCache::default()), 0)
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
    store.create_task(None, None, None, None);
    store.create_task(None, None, None, None);

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
    let t1 = store.create_task(None, None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store.create_task(None, None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({"status": "working"}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 1, "Should list 1 working task");
    assert_eq!(tasks[0]["status"]["state"], "WORKING");
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
