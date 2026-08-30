use std::sync::Arc;

use crate::{A2aError, AgentCapabilities, AgentCard, Message, Part, PeerCache, Role, TaskState};

use super::{A2aClient, cancel_task, get_agent_card, get_task, list_tasks, send_task};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Crypto provider
// ---------------------------------------------------------------------------

static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Start a test server, return (server, client, server_url).
async fn test_setup() -> (crate::A2aServer, A2aClient, String) {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "test-server".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        capabilities: AgentCapabilities::default(),
        skills: vec![],
        ..Default::default()
    };
    let server = crate::A2aServer::start(card, Arc::new(PeerCache::default()), 0)
        .await
        .unwrap();
    let client = A2aClient::new().unwrap();
    let url = server.local_url.clone();
    (server, client, url)
}

// ---------------------------------------------------------------------------
// send_task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_task() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert!(!task.id.is_empty(), "Task should have an ID");
    assert_eq!(
        task.status.state,
        TaskState::Working,
        "New task should be in Working state"
    );
    // UUID format: 36 chars
    assert_eq!(task.id.len(), 36, "Task ID should be a UUID");
    Ok(())
}

#[tokio::test]
async fn test_send_task_with_session() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(&client, &url, msg, Some("sess-1".into()), None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.session_id, Some("sess-1".into()));
    Ok(())
}

#[tokio::test]
async fn test_send_task_with_sender_url() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(
        &client,
        &url,
        msg,
        None,
        Some("http://me.local:12345".into()),
    )
    .await
    .map_err(|e| format!("{e:?}"))?;

    assert!(!task.id.is_empty(), "Task should have an ID");
    Ok(())
}

#[tokio::test]
async fn test_send_task_connection_refused() {
    ensure_crypto_provider();
    let client = A2aClient::new().unwrap();
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let result = send_task(&client, "http://127.0.0.1:1", msg, None, None).await;
    assert!(
        matches!(result, Err(A2aError::ConnectionRefused(_))),
        "Expected ConnectionRefused, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// get_task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_task() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let retrieved = get_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(retrieved.id, sent.id);
    assert_eq!(retrieved.status.state, TaskState::Working);
    Ok(())
}

#[tokio::test]
async fn test_get_task_not_found() {
    let (_server, client, url) = test_setup().await;
    let result = get_task(&client, &url, "nonexistent-id").await;
    assert!(
        matches!(result, Err(A2aError::TaskNotFound(_))),
        "Expected TaskNotFound, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// cancel_task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cancel_task() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "cancel me".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let canceled = cancel_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.status.state, TaskState::Canceled);
    Ok(())
}

#[tokio::test]
async fn test_cancel_already_completed() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "cancel me".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // First cancel should succeed
    let first = cancel_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.status.state, TaskState::Canceled);

    // Second cancel should fail — already Canceled
    let result = cancel_task(&client, &url, &sent.id).await;
    assert!(
        matches!(result, Err(A2aError::InvalidStateTransition { .. })),
        "Second cancel should return InvalidStateTransition, got: {result:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// get_agent_card
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_agent_card() -> Result<()> {
    let (_server, client, url) = test_setup().await;
    let card = get_agent_card(&client, &url)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(card.name, "test-server");
    assert_eq!(card.version, "1.0");
    Ok(())
}

#[tokio::test]
async fn test_get_agent_card_connection_refused() {
    ensure_crypto_provider();
    let client = A2aClient::new().unwrap();
    let result = get_agent_card(&client, "http://127.0.0.1:1").await;
    assert!(matches!(result, Err(A2aError::ConnectionRefused(_))));
}

// ---------------------------------------------------------------------------
// list_tasks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_tasks() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    // Send a couple of tasks
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "first".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let msg2 = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "second".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    send_task(&client, &url, msg2, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // List all tasks
    let tasks = list_tasks(&client, &url, None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(tasks.len(), 2, "Should list 2 tasks");
    Ok(())
}

#[tokio::test]
async fn test_list_tasks_filtered() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    // Send a task (it starts Submitted then transitions to Working)
    let send_resp = send_task(
        &client,
        &url,
        Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "to-cancel".into(),
            }],
            message_id: uuid::Uuid::new_v4().to_string(),
            extensions: None,
            metadata: None,
        },
        None,
        None,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;

    let msg2 = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "keep".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    send_task(&client, &url, msg2, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Cancel the first task so it's in 'canceled' state
    cancel_task(&client, &url, &send_resp.id)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // List only working tasks
    let working = list_tasks(&client, &url, Some(TaskState::Working))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(working.len(), 1, "Should have 1 working task");
    assert_eq!(working[0].status.state, TaskState::Working);

    // List only canceled tasks
    let canceled = list_tasks(&client, &url, Some(TaskState::Canceled))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.len(), 1, "Should have 1 canceled task");
    assert_eq!(canceled[0].status.state, TaskState::Canceled);
    Ok(())
}

// ---------------------------------------------------------------------------
// Full lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_lifecycle() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    // Send
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "lifecycle".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(sent.status.state, TaskState::Working);

    // Get
    let retrieved = get_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(retrieved.id, sent.id);

    // Cancel
    let canceled = cancel_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // Get after cancel
    let final_task = get_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(final_task.status.state, TaskState::Canceled);
    Ok(())
}

// ---------------------------------------------------------------------------
// Edge cases: trailing slash in target_url
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_task_with_trailing_slash() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let url_with_slash = format!("{}/", url.trim_end_matches('/'));

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "trailing slash".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(&client, &url_with_slash, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(!task.id.is_empty(), "Task should have an ID");
    Ok(())
}
