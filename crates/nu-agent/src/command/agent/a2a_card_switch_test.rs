use nu_agent_a2a::{AgentBuilder, Peer, Skill, rebuild_card_for_switch, skill_from_persona};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// The workspace reqwest is built with `rustls-no-provider`, meaning the
// application must install a crypto provider before constructing a Client.
static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Create a [`reqwest::Client`] that sends `A2A-Version: 1.0` on every
/// request, matching what the middleware expects on A2A API paths.
fn test_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::HeaderName::from_static("a2a-version"),
        reqwest::header::HeaderValue::from_static(nu_agent_a2a::A2A_VERSION),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

/// Integration test: start an A2A server via AgentBuilder, simulate an agent
/// switch by writing a new card through the card_handle, then GET the card
/// and assert it reflects the new agent.
///
/// This exercises the full plumbing path that `mode_execute.rs` uses:
///   AgentBuilder::build → card_handle → rebuild_card_for_switch → server
///   serves updated card.
#[tokio::test]
async fn test_a2a_card_updated_on_agent_switch() -> Result<()> {
    ensure_crypto_provider();

    // ── 1. Build an agent with initial card "Agent A" ─────────────────────
    let initial_skills = vec![Skill {
        id: "agent-a".into(),
        name: "Agent A".into(),
        description: "The first agent".into(),
        inputs: None,
        outputs: None,
    }];

    let handle = AgentBuilder::new("Agent A")
        .description("Initial agent description")
        .skills(initial_skills)
        .port(0)
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    let local_url = handle.server.local_url.clone();
    let client = test_client();

    // ── 2. GET initial card — assert name is "Agent A" ───────────────────
    let resp = client
        .get(format!("{}/.well-known/agent-card.json", local_url))
        .send()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(body["name"], "Agent A");
    assert_eq!(body["description"], "Initial agent description");
    assert_eq!(
        body["skills"]
            .as_array()
            .ok_or("skills should be an array")?
            .len(),
        1
    );
    assert_eq!(body["skills"][0]["name"], "Agent A");

    // Capture server-bound fields before the switch
    let initial_url = body["url"]
        .as_str()
        .ok_or("url should be a string")?
        .to_string();
    let initial_version = body["version"]
        .as_str()
        .ok_or("version should be a string")?
        .to_string();

    // ── 3. Simulate an agent switch via the card_handle ──────────────────
    // This replicates the exact closure pattern from mode_execute.rs:288-312:
    //   let on_agent_switch = a2a.card_handle.map(|card_handle| {
    //       let cache = a2a.cache.clone();
    //       let self_port = a2a.self_port;
    //       Arc::new(move |name, description| {
    //           let mut card = card_handle.write().expect("agent_card lock");
    //           let old_name = card.name.clone();
    //           let skill = skill_from_persona(&name, description.as_deref());
    //           let new_card = rebuild_card_for_switch(&card, &name, description.as_deref(), vec![skill]);
    //           *card = new_card.clone();
    //           if let (Some(ref cache), Some(port)) = (cache.clone(), self_port) {
    //               cache.remove(&old_name);
    //               cache.add_or_update(Peer { ... });
    //           }
    //       })
    //   });
    let cache = handle.cache();
    let self_port = handle.server.port;
    let old_name: String;
    {
        let card_handle = handle.card_handle().ok_or("card_handle should be Some")?;
        let mut card = card_handle.write().expect("agent_card lock");
        old_name = card.name.clone();
        let skill = skill_from_persona("Agent B", Some("Switched agent description"));
        let new_card = rebuild_card_for_switch(
            &card,
            "Agent B",
            Some("Switched agent description"),
            vec![skill],
        );
        *card = new_card.clone();

        // Update the peer cache self-entry so agent_list reflects the new name.
        cache.remove(&old_name);
        cache.add_or_update(Peer {
            name: "Agent B".to_string(),
            url: card.url.clone(),
            host: "127.0.0.1".to_string(),
            port: self_port,
            card: Some(card.clone()),
            discovered_at: std::time::Instant::now(),
        });
    }

    // ── 4. GET card again — assert name is now "Agent B" ─────────────────
    let resp = client
        .get(format!("{}/.well-known/agent-card.json", local_url))
        .send()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(body["name"], "Agent B");
    assert_eq!(body["description"], "Switched agent description");
    assert_eq!(
        body["skills"]
            .as_array()
            .ok_or("skills should be an array")?
            .len(),
        1
    );
    assert_eq!(body["skills"][0]["name"], "Agent B");
    assert_eq!(
        body["skills"][0]["description"],
        "Switched agent description"
    );

    // ── 5. Assert the peer cache is updated ──────────────────────────────
    // The old entry must be gone and the new entry must exist with the
    // updated card. This verifies the cache-update logic from
    // mode_execute.rs:300-309.
    {
        let peers = cache.list();
        let old_peer = peers.iter().find(|p| p.name == "Agent A");
        assert!(
            old_peer.is_none(),
            "old peer 'Agent A' must be removed from cache"
        );
        let new_peer = peers
            .iter()
            .find(|p| p.name == "Agent B")
            .ok_or("new peer 'Agent B' must exist in cache")?;
        let cached_card = new_peer
            .card
            .as_ref()
            .ok_or("cached peer must have a card")?;
        assert_eq!(cached_card.name, "Agent B");
        assert_eq!(
            cached_card.description.as_deref(),
            Some("Switched agent description")
        );
    }

    // ── 6. Assert server-bound fields are preserved ──────────────────────
    // The url is updated by AgentBuilder::build after server start, so it
    // will be the real local_url, not the placeholder.
    assert_eq!(
        body["url"], initial_url,
        "server-bound url must be preserved"
    );
    assert_eq!(
        body["version"], initial_version,
        "server-bound version must be preserved"
    );

    // ── 6. Clean up ──────────────────────────────────────────────────────
    handle.shutdown().await;
    Ok(())
}
