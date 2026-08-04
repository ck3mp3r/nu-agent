use std::sync::Arc;

use super::*;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Tool definition tests (sync)
// ---------------------------------------------------------------------------

#[test]
fn test_tool_defs_returns_six_tools() {
    let defs = a2a_tool_defs();
    assert_eq!(defs.len(), 6, "Should have 6 tool definitions");
}

#[test]
fn test_agent_list_has_no_parameters() {
    let defs = a2a_tool_defs();
    let tool = defs.iter().find(|t| t.name == "agent_list").unwrap();
    let props = tool.parameters["properties"].as_object().unwrap();
    assert!(
        props.is_empty(),
        "agent_list should have no parameters (no filter) — LLM should get all agents"
    );
}

#[test]
fn test_agent_get_card_requires_name() {
    let defs = a2a_tool_defs();
    let tool = defs.iter().find(|t| t.name == "agent_getCard").unwrap();
    let required = tool.parameters["required"].as_array().unwrap();
    assert!(required.contains(&Value::String("name".into())));
}

#[test]
fn test_tasks_send_requires_target_and_text() {
    let defs = a2a_tool_defs();
    let tool = defs.iter().find(|t| t.name == "tasks_send").unwrap();
    let required = tool.parameters["required"].as_array().unwrap();
    assert!(required.contains(&Value::String("target".into())));
    assert!(required.contains(&Value::String("text".into())));
}

#[test]
fn test_tasks_get_requires_task_id_and_target() {
    let defs = a2a_tool_defs();
    let tool = defs.iter().find(|t| t.name == "tasks_get").unwrap();
    let required = tool.parameters["required"].as_array().unwrap();
    assert!(required.contains(&Value::String("taskId".into())));
    assert!(required.contains(&Value::String("target".into())));
}

#[test]
fn test_tasks_cancel_requires_task_id_and_target() {
    let defs = a2a_tool_defs();
    let tool = defs.iter().find(|t| t.name == "tasks_cancel").unwrap();
    let required = tool.parameters["required"].as_array().unwrap();
    assert!(required.contains(&Value::String("taskId".into())));
    assert!(required.contains(&Value::String("target".into())));
}

#[test]
fn test_tasks_list_has_optional_status_param() {
    let defs = a2a_tool_defs();
    let tool = defs.iter().find(|t| t.name == "tasks_list").unwrap();
    assert!(
        tool.parameters.get("required").is_none(),
        "tasks.list should have no required params"
    );
    let properties = tool.parameters["properties"].as_object().unwrap();
    assert!(
        properties.contains_key("status"),
        "tasks.list should have optional status param"
    );
    assert_eq!(properties["status"]["type"], "string");
    assert!(
        properties.contains_key("target"),
        "tasks.list should have optional target param"
    );
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
async fn test_handle_agent_list_empty() {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::new()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let result = handle_agent_list(ctx, serde_json::json!({})).await.unwrap();
    assert_eq!(result["agents"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_handle_agent_list_with_peers() {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::new()),
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
    let result = handle_agent_list(ctx, serde_json::json!({})).await.unwrap();
    assert_eq!(result["agents"].as_array().unwrap().len(), 1);
    assert_eq!(result["agents"][0]["name"], "alice");
}

#[tokio::test]
async fn test_handle_agent_get_card_not_found() {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::new()),
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
        cache: Arc::new(PeerCache::new()),
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
async fn test_handle_tasks_send_to_real_server() {
    ensure_crypto_provider();
    // Start real server, add to cache, send task via handler
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()), 0)
        .await
        .unwrap();

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::new()),
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
    let result = handle_tasks_send(ctx.clone(), params).await.unwrap();
    assert!(result.get("taskId").is_some(), "Should have a taskId");
    assert_eq!(
        result["status"], "sent",
        "tasks.send now returns status 'sent' (fire-and-forget)"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_handle_tasks_send_with_own_card_url() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()), 0)
        .await
        .unwrap();

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::new()),
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
    let result = handle_tasks_send(ctx.clone(), params).await.unwrap();
    assert!(result.get("taskId").is_some(), "Should have a taskId");

    server.shutdown().await;
}

#[tokio::test]
async fn test_handle_tasks_get_to_real_server() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()), 0)
        .await
        .unwrap();

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::new()),
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
    let send_result = handle_tasks_send(ctx.clone(), send_params).await.unwrap();
    let task_id = send_result["taskId"].as_str().unwrap().to_string();

    // Then get it
    let get_params = serde_json::json!({"target": "test-agent", "taskId": task_id});
    let get_result = handle_tasks_get(ctx, get_params).await.unwrap();
    assert_eq!(get_result["taskId"], task_id);

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// tasks.list handler
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handle_tasks_list_with_local_store() {
    ensure_crypto_provider();
    let store = Arc::new(InMemoryTaskStore::new());
    store.create_task(None, None, None, None);
    store.create_task(None, None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::new()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({})).await.unwrap();
    let tasks = result["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2, "Should list 2 tasks from local store");
}

#[tokio::test]
async fn test_handle_tasks_list_with_local_store_filtered() {
    ensure_crypto_provider();
    let store = Arc::new(InMemoryTaskStore::new());
    let t1 = store.create_task(None, None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .unwrap();
    store.create_task(None, None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::new()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({"status": "working"}))
        .await
        .unwrap();
    let tasks = result["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1, "Should list 1 working task");
    assert_eq!(tasks[0]["status"]["state"], "WORKING");
}

#[tokio::test]
async fn test_handle_tasks_list_no_store() {
    ensure_crypto_provider();
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::new()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({})).await.unwrap();
    let tasks = result["tasks"].as_array().unwrap();
    assert!(tasks.is_empty(), "Should return empty list when no store");
}

#[tokio::test]
async fn agent_list_excludes_self() {
    ensure_crypto_provider();
    let own_url = "http://127.0.0.1:9999".to_string();
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::new()),
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

    let result = handle_agent_list(ctx, serde_json::json!({})).await.unwrap();
    let agents = result["agents"].as_array().unwrap();

    // Self should be excluded — only the other agent remains
    assert_eq!(agents.len(), 1, "self-agent should be excluded from output");

    let other_agent = &agents[0];
    assert_eq!(other_agent["name"], "other-agent");

    // is_self field must NOT be present in any entry
    for agent in agents {
        assert!(
            !agent.as_object().unwrap().contains_key("is_self"),
            "no entry should have an is_self field"
        );
    }
}
