use std::time::Duration;

use super::*;

// The workspace reqwest is built with `rustls-no-provider`, meaning the
// application must install a crypto provider before constructing a Client.
static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// AgentHandle::start
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_start_shutdown() {
    ensure_crypto_provider();

    let handle = AgentHandle::start("test-agent", Some("A test agent"), vec![], 0, "test-mesh".into())
        .await
        .unwrap();

    assert!(handle.server.port > 0, "Server should be on a real port");
    assert_eq!(
        handle.card.url, handle.server.local_url,
        "Card URL should match server URL"
    );
    assert_eq!(handle.card.name, "test-agent");
    assert_eq!(handle.card.description.as_deref(), Some("A test agent"));

    // Health endpoint should respond
    let resp = reqwest::get(&format!("{}/health", handle.server.local_url))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let port = handle.server.port;
    handle.shutdown().await;

    // Give time for port release from TIME_WAIT
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Port should be free after shutdown
    tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .expect("Port should be free after shutdown");
}

#[tokio::test]
async fn test_agent_card_reflects_persona() {
    ensure_crypto_provider();

    let skills = vec![Skill {
        id: "coding".into(),
        name: "Coding".into(),
        description: "Writes Rust code".into(),
        inputs: None,
        outputs: None,
    }];

    let handle = AgentHandle::start("coder", Some("A coding agent"), skills, 0, "test-mesh".into())
        .await
        .unwrap();

    let client = A2aClient::new();
    let card = client
        .get_agent_card(&handle.server.local_url)
        .await
        .unwrap();

    assert_eq!(card.name, "coder");
    assert_eq!(card.description.unwrap(), "A coding agent");
    assert_eq!(card.skills.len(), 1);
    assert_eq!(card.skills[0].id, "coding");

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_empty_skills() {
    ensure_crypto_provider();

    let handle = AgentHandle::start("no-skills", Some("Has no explicit skills"), vec![], 0, "test-mesh".into())
        .await
        .unwrap();

    let client = A2aClient::new();
    let card = client
        .get_agent_card(&handle.server.local_url)
        .await
        .unwrap();
    assert!(card.skills.is_empty(), "Skills should be empty");

    handle.shutdown().await;
}

#[tokio::test]
async fn test_agent_no_description() {
    ensure_crypto_provider();

    let handle = AgentHandle::start("no-desc", None, vec![], 0, "test-mesh".into())
        .await
        .unwrap();

    let client = A2aClient::new();
    let card = client
        .get_agent_card(&handle.server.local_url)
        .await
        .unwrap();
    assert!(card.description.is_none());

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// AgentHandle::start_with_card
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_start_with_card() {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "custom-card".into(),
        url: "http://127.0.0.1:0".into(),
        version: "2.0".into(),
        skills: vec![],
        ..Default::default()
    };

    let handle = AgentHandle::start_with_card(card, 0, "test-mesh".into()).await.unwrap();
    assert_eq!(handle.card.name, "custom-card");
    assert_eq!(handle.card.version, "2.0");
    assert!(
        handle.card.url.contains(&handle.server.port.to_string()),
        "Card URL should contain actual port"
    );

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// End-to-end: two-agent scenario
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_two_agents_start_independently() {
    ensure_crypto_provider();

    let agent_a = AgentHandle::start("agent-a", Some("First agent"), vec![], 0, "test-mesh".into())
        .await
        .unwrap();
    let agent_b = AgentHandle::start("agent-b", Some("Second agent"), vec![], 0, "test-mesh".into())
        .await
        .unwrap();

    // Both servers should be on different ports
    assert!(agent_a.server.port > 0);
    assert!(agent_b.server.port > 0);
    assert_ne!(
        agent_a.server.port, agent_b.server.port,
        "Agents should be on different ports"
    );

    // Both should serve their cards correctly
    let client = A2aClient::new();
    let card_a = client
        .get_agent_card(&agent_a.server.local_url)
        .await
        .unwrap();
    let card_b = client
        .get_agent_card(&agent_b.server.local_url)
        .await
        .unwrap();
    assert_eq!(card_a.name, "agent-a");
    assert_eq!(card_b.name, "agent-b");
    assert_eq!(card_a.description.as_deref(), Some("First agent"));
    assert_eq!(card_b.description.as_deref(), Some("Second agent"));

    // Task lifecycle across agents: send, get, cancel on each
    let msg_a = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello from a".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let msg_b = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello from b".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task_a = client
        .send_task(&agent_a.server.local_url, msg_a, None, None)
        .await
        .unwrap();
    let task_b = client
        .send_task(&agent_b.server.local_url, msg_b, None, None)
        .await
        .unwrap();

    assert_eq!(task_a.status.state, TaskState::Working);
    assert_eq!(task_b.status.state, TaskState::Working);

    // Cancel both
    let canceled_a = client
        .cancel_task(&agent_a.server.local_url, &task_a.id)
        .await
        .unwrap();
    let canceled_b = client
        .cancel_task(&agent_b.server.local_url, &task_b.id)
        .await
        .unwrap();
    assert_eq!(canceled_a.status.state, TaskState::Canceled);
    assert_eq!(canceled_b.status.state, TaskState::Canceled);

    agent_a.shutdown().await;
    agent_b.shutdown().await;
}
