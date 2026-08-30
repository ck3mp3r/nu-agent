use super::{A2aHttpClient, MockHttpClient, get_agent_card, get_task, send_task};
use crate::A2aError;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// MockHttpClient basic usage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_expect_post_ok() -> Result<()> {
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/message:send",
        serde_json::json!({"task": {"id": "task-1", "status": {"state": "WORKING", "timestamp": "2026-01-01T00:00:00Z"}, "artifacts": []}}),
    );

    let result = mock
        .post_json("http://example.com/message:send", serde_json::json!({}))
        .await;
    assert!(result.is_ok());
    let task = result.map_err(|e| format!("{e:?}"))?;
    assert_eq!(task["task"]["id"], "task-1");
    Ok(())
}

#[tokio::test]
async fn test_mock_expect_post_error() {
    let mock = MockHttpClient::default();
    mock.expect_post_error(
        "http://example.com/tasks:list",
        A2aError::TaskNotFound("no tasks".into()),
    );

    let result = mock
        .post_json("http://example.com/tasks:list", serde_json::json!({}))
        .await;
    assert!(
        matches!(result, Err(A2aError::TaskNotFound(_))),
        "Expected TaskNotFound, got {result:?}"
    );
}

#[tokio::test]
async fn test_mock_expect_get_ok() -> Result<()> {
    let mock = MockHttpClient::default();
    let card_json = br#"{"name":"test-agent","version":"1.0","capabilities":{"streaming":true,"pushNotifications":false,"stateful":true,"extendedAgentCard":false}}"#;
    mock.expect_get_ok(
        "http://example.com/.well-known/agent-card.json",
        card_json.to_vec(),
    );

    let result = mock
        .get_bytes("http://example.com/.well-known/agent-card.json")
        .await;
    assert!(result.is_ok());
    let bytes = result.map_err(|e| format!("{e:?}"))?;
    assert_eq!(bytes, card_json);
    Ok(())
}

#[tokio::test]
async fn test_mock_expect_get_error() {
    let mock = MockHttpClient::default();
    mock.expect_get_error(
        "http://example.com/tasks/missing",
        A2aError::TaskNotFound("missing".into()),
    );

    let result = mock.get_bytes("http://example.com/tasks/missing").await;
    assert!(
        matches!(result, Err(A2aError::TaskNotFound(_))),
        "Expected TaskNotFound, got {result:?}"
    );
}

#[tokio::test]
async fn test_mock_no_response_registered() {
    let mock = MockHttpClient::default();

    let result = mock
        .post_json("http://example.com/unknown", serde_json::json!({}))
        .await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("no mock response registered"),
        "Should get 'no mock response registered' error"
    );

    let result2 = mock.get_bytes("http://example.com/unknown").await;
    assert!(result2.is_err());
    assert!(
        result2
            .unwrap_err()
            .to_string()
            .contains("no mock response registered"),
        "Should get 'no mock response registered' error"
    );
}

#[tokio::test]
async fn test_mock_method_routing() -> Result<()> {
    // Same URL, different methods -> different responses
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/api",
        serde_json::json!({"method": "POST"}),
    );
    mock.expect_get_ok("http://example.com/api", b"GET response".to_vec());

    let post_result = mock
        .post_json("http://example.com/api", serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(post_result["method"], "POST");

    let get_result = mock
        .get_bytes("http://example.com/api")
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(get_result, b"GET response");
    Ok(())
}

// ---------------------------------------------------------------------------
// Free functions with MockHttpClient
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_task_with_mock() -> Result<()> {
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/message:send",
        serde_json::json!({
            "task": {
                "id": "00000000-0000-0000-0000-000000000001",
                "status": {
                    "state": "WORKING",
                    "timestamp": "2026-01-01T00:00:00Z"
                },
                "artifacts": []
            }
        }),
    );

    let msg = crate::Message {
        role: crate::Role::User,
        parts: vec![crate::Part::Text {
            text: "hello".into(),
        }],
        message_id: "msg-1".into(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(&mock, "http://example.com", msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.id, "00000000-0000-0000-0000-000000000001");
    assert_eq!(task.status.state, crate::TaskState::Working);
    Ok(())
}

#[tokio::test]
async fn test_get_task_with_mock() -> Result<()> {
    let mock = MockHttpClient::default();
    // get_task constructs URL: http://example.com/tasks/task-1
    let response = serde_json::json!({
        "task": {
            "id": "task-1",
            "status": {
                "state": "WORKING",
                "timestamp": "2026-01-01T00:00:00Z"
            },
            "artifacts": []
        }
    });
    let bytes = serde_json::to_vec(&response).unwrap();
    mock.expect_get_ok("http://example.com/tasks/task-1", bytes);

    let task = get_task(&mock, "http://example.com", "task-1")
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.id, "task-1");
    Ok(())
}

#[tokio::test]
async fn test_get_agent_card_with_mock() -> Result<()> {
    let mock = MockHttpClient::default();
    let card = crate::AgentCard {
        name: "mock-agent".into(),
        url: "http://example.com".into(),
        version: "2.0".into(),
        capabilities: crate::AgentCapabilities::default(),
        skills: vec![],
        ..Default::default()
    };
    let card_json = serde_json::to_vec(&card).unwrap();
    mock.expect_get_ok("http://example.com/.well-known/agent-card.json", card_json);

    let result = get_agent_card(&mock, "http://example.com")
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.name, "mock-agent");
    assert_eq!(result.version, "2.0");
    Ok(())
}
