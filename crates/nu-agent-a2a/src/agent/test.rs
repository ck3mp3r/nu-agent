use std::time::Duration;

use crate::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// The workspace reqwest is built with `rustls-no-provider`, meaning the
// application must install a crypto provider before constructing a Client.
static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// AgentBuilder::build
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_start_shutdown() -> Result<()> {
    ensure_crypto_provider();

    let handle = AgentBuilder::new("test-agent")
        .description("A test agent")
        .port(0)
        .mesh_key("test-mesh".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert!(handle.server.port > 0, "Server should be on a real port");
    assert_eq!(
        handle.card().url,
        handle.server.local_url,
        "Card URL should match server URL"
    );
    assert!(
        handle.card().name.starts_with("test-agent-"),
        "card name should include port suffix, got '{}'",
        handle.card().name,
    );
    assert_eq!(handle.card().description.as_deref(), Some("A test agent"));

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
        .map_err(|e| format!("port should be free after shutdown: {e:?}"))?;
    Ok(())
}

#[tokio::test]
async fn test_agent_card_reflects_persona() -> Result<()> {
    ensure_crypto_provider();

    let skills = vec![Skill {
        id: "coding".into(),
        name: "Coding".into(),
        description: "Writes Rust code".into(),
        inputs: None,
        outputs: None,
    }];

    let handle = AgentBuilder::new("coder")
        .description("A coding agent")
        .skills(skills)
        .port(0)
        .mesh_key("test-mesh".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    let client = A2aClient::new().unwrap();
    let card = get_agent_card(&client, &handle.server.local_url)
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(card.name, "coder");
    let description = card.description.ok_or("card should have description")?;
    assert_eq!(description, "A coding agent");
    assert_eq!(card.skills.len(), 1);
    assert_eq!(card.skills[0].id, "coding");

    handle.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_empty_skills() -> Result<()> {
    ensure_crypto_provider();

    let handle = AgentBuilder::new("no-skills")
        .description("Has no explicit skills")
        .port(0)
        .mesh_key("test-mesh".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    let client = A2aClient::new().unwrap();
    let card = get_agent_card(&client, &handle.server.local_url)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(card.skills.is_empty(), "Skills should be empty");

    handle.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_agent_no_description() -> Result<()> {
    ensure_crypto_provider();

    let handle = AgentBuilder::new("no-desc")
        .port(0)
        .mesh_key("test-mesh".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    let client = A2aClient::new().unwrap();
    let card = get_agent_card(&client, &handle.server.local_url)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(card.description.is_none());

    handle.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// AgentBuilder::with_card
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_start_with_card() -> Result<()> {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "custom-card".into(),
        url: "http://127.0.0.1:0".into(),
        version: "2.0".into(),
        skills: vec![],
        ..Default::default()
    };

    let handle = AgentBuilder::new("custom-card")
        .with_card(card)
        .port(0)
        .mesh_key("test-mesh".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        handle.card().name.starts_with("custom-card-"),
        "card name should include port suffix, got '{}'",
        handle.card().name,
    );
    assert_eq!(handle.card().version, "2.0");
    assert!(
        handle.card().url.contains(&handle.server.port.to_string()),
        "Card URL should contain actual port"
    );

    handle.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// End-to-end: two-agent scenario
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_two_agents_start_independently() -> Result<()> {
    ensure_crypto_provider();

    let agent_a = AgentBuilder::new("agent-a")
        .description("First agent")
        .port(0)
        .mesh_key("test-mesh".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;
    let agent_b = AgentBuilder::new("agent-b")
        .description("Second agent")
        .port(0)
        .mesh_key("test-mesh".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Both servers should be on different ports
    assert!(agent_a.server.port > 0);
    assert!(agent_b.server.port > 0);
    assert_ne!(
        agent_a.server.port, agent_b.server.port,
        "Agents should be on different ports"
    );

    // Both should serve their cards correctly
    let client = A2aClient::new().unwrap();
    let card_a = get_agent_card(&client, &agent_a.server.local_url)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let card_b = get_agent_card(&client, &agent_b.server.local_url)
        .await
        .map_err(|e| format!("{e:?}"))?;
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

    let task_a = send_task(&client, &agent_a.server.local_url, msg_a, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let task_b = send_task(&client, &agent_b.server.local_url, msg_b, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(task_a.status.state, TaskState::Working);
    assert_eq!(task_b.status.state, TaskState::Working);

    // Cancel both
    let canceled_a = cancel_task(&client, &agent_a.server.local_url, &task_a.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let canceled_b = cancel_task(&client, &agent_b.server.local_url, &task_b.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled_a.status.state, TaskState::Canceled);
    assert_eq!(canceled_b.status.state, TaskState::Canceled);

    agent_a.shutdown().await;
    agent_b.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-discovery: agent appears in its own peer cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_discovers_self() -> Result<()> {
    ensure_crypto_provider();

    let handle = AgentBuilder::new("self-aware-agent")
        .description("An agent that knows itself")
        .port(0)
        .mesh_key("test-mesh".to_string())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    let peers = handle.cache().list();
    let self_peer = peers.iter().find(|p| p.name.contains("self-aware-agent"));
    assert!(
        self_peer.is_some(),
        "Agent should find itself in the peer cache, got peers: {peers:?}"
    );

    let peer = self_peer.ok_or("should have self peer")?;
    assert_eq!(
        peer.url,
        format!("http://127.0.0.1:{}", handle.server.port),
        "Self peer URL should match server local_url"
    );
    assert_eq!(peer.port, handle.server.port);
    assert!(
        peer.card.is_some(),
        "Self peer should have its AgentCard attached"
    );

    if let Some(ref card) = peer.card {
        assert!(
            card.name.contains("self-aware-agent"),
            "card name '{}' should contain 'self-aware-agent'",
            card.name
        );
        assert_eq!(
            card.description.as_deref(),
            Some("An agent that knows itself")
        );
        // After 9.4: name might be "self-aware-agent-{port}"
        // The test handles both cases.
        assert!(
            card.name.ends_with(&format!("-{}", handle.server.port))
                || card.name == "self-aware-agent",
            "card name '{}' should have port suffix or be exact match",
            card.name,
        );
    }

    handle.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// mDNS instance naming: port suffix for auto-named agents
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mdns_name_appends_port_when_auto() -> Result<()> {
    ensure_crypto_provider();

    let builder = AgentBuilder::new("researcher")
        .port(0)
        .mesh_key("test".to_string());
    let handle = builder.build().await.map_err(|e| format!("{e:?}"))?;
    let suffix = format!("-{}", handle.server.port);
    assert!(
        handle.card().name.ends_with(&suffix),
        "expected card.name 'researcher-{{port}}', got '{}'",
        handle.card().name,
    );
    handle.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_mdns_name_uses_exact_name_when_explicit() -> Result<()> {
    ensure_crypto_provider();

    let builder = AgentBuilder::new("my-custom-agent")
        .has_explicit_name(true)
        .port(0)
        .mesh_key("test".to_string());
    let handle = builder.build().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        handle.card().name,
        "my-custom-agent",
        "explicit name should not have port suffix"
    );
    handle.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Periodic mDNS re-registration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reregister_token_is_set_after_build() -> Result<()> {
    ensure_crypto_provider();
    let handle = AgentBuilder::new("test-rereg-token")
        .discovery(PeerDiscoveryImpl::Noop)
        .port(0)
        .mesh_key("test".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(handle.reregister_token().is_some());
    handle.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn reregister_task_does_not_block_startup() -> Result<()> {
    ensure_crypto_provider();
    let handle = AgentBuilder::new("test-rereg-noblock")
        .discovery(PeerDiscoveryImpl::Noop)
        .port(0)
        .mesh_key("test".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(handle.server.port > 0);
    let resp = reqwest::get(&format!("{}/health", handle.server.local_url))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    handle.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn shutdown_cancels_reregister_without_panic() -> Result<()> {
    ensure_crypto_provider();
    let handle = AgentBuilder::new("test-rereg-cancel")
        .discovery(PeerDiscoveryImpl::Noop)
        .port(0)
        .mesh_key("test".into())
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;
    handle.shutdown().await;
    Ok(())
}
