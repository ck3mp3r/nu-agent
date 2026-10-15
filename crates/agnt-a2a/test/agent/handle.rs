use super::*;

// ---------------------------------------------------------------------------
// AgentHandle accessors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handle_accessors_expose_server_state() -> Result<()> {
    let handle = AgentBuilder::new("handle-accessors")
        .discovery(PeerDiscoveryImpl::Noop)
        .port(0)
        .mesh_key("mesh-handle".into())
        .blocking_timeout(TEST_BLOCKING_TIMEOUT)
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    // card_handle exposes the server's card; the handle's own card is that
    // card with the auto-generated port suffix applied.
    let card_handle = handle.card_handle().ok_or("card_handle should be Some")?;
    let server_card_name = card_handle.read().expect("card lock").name.clone();
    assert_eq!(
        handle.card().name,
        format!("{server_card_name}-{}", handle.server.port),
        "the handle card name is the server card name plus the port suffix"
    );

    // discovery_handle exposes the injected discovery implementation.
    let discovery = handle.discovery_handle();
    assert!(
        discovery
            .lock()
            .expect("discovery lock")
            .fullname()
            .is_none(),
        "Noop discovery has no mDNS fullname"
    );

    // mesh_key is the key the builder was given.
    assert_eq!(handle.mesh_key(), "mesh-handle");

    // task_store starts empty and is the server's store.
    let store = handle.task_store();
    assert!(store.list_tasks(None).is_empty());

    // a2a_tool_context carries the store, completion sender, and card.
    let ctx = handle.a2a_tool_context(tokio::runtime::Handle::current());
    assert!(
        ctx.task_store.is_some(),
        "tool context must carry the task store"
    );
    assert!(
        ctx.completion_tx.is_some(),
        "tool context must carry the completion sender"
    );
    assert!(ctx.runtime_handle.is_some());
    assert_eq!(ctx.own_card.name, handle.card().name);

    handle.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_take_receivers_once() -> Result<()> {
    let mut handle = AgentBuilder::new("handle-receivers")
        .discovery(PeerDiscoveryImpl::Noop)
        .port(0)
        .mesh_key("mesh".into())
        .blocking_timeout(TEST_BLOCKING_TIMEOUT)
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert!(
        handle.take_completion_receiver().is_some(),
        "first take must yield the completion receiver"
    );
    assert!(
        handle.take_completion_receiver().is_none(),
        "second take must yield None"
    );
    assert!(
        handle.take_task_cancel_receiver().is_some(),
        "first take must yield the cancel receiver"
    );
    assert!(
        handle.take_task_cancel_receiver().is_none(),
        "second take must yield None"
    );

    handle.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// AgentBuilder — failure and discovery wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_builder_bind_failure_returns_error() -> Result<()> {
    // Hold the port so the builder's bind fails.
    let blocker = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = blocker.local_addr()?.port();

    let result = AgentBuilder::new("bind-fail")
        .discovery(PeerDiscoveryImpl::Noop)
        .port(port)
        .mesh_key("mesh".into())
        .blocking_timeout(TEST_BLOCKING_TIMEOUT)
        .build()
        .await;

    let (a2a_err, server) = result
        .err()
        .ok_or("build must fail when the port is taken")?;
    assert!(
        server.is_none(),
        "no server handle is returned when the bind fails"
    );
    assert!(
        format!("{a2a_err:?}").contains("bind failed"),
        "error must report the bind failure, got: {a2a_err:?}"
    );

    drop(blocker);
    Ok(())
}

#[tokio::test]
async fn test_builder_static_discovery_populates_peer_cache() -> Result<()> {
    let peer = Peer {
        name: "static-peer".into(),
        url: "http://127.0.0.1:5555".into(),
        host: "127.0.0.1".into(),
        port: 5555,
        card: None,
        discovered_at: std::time::Instant::now(),
    };

    let handle = AgentBuilder::new("static-discovery")
        .discovery(PeerDiscoveryImpl::Static(
            crate::discovery::static_discovery::StaticPeerDiscovery::new(vec![peer]),
        ))
        .port(0)
        .mesh_key("mesh".into())
        .blocking_timeout(TEST_BLOCKING_TIMEOUT)
        .build()
        .await
        .map_err(|e| format!("{e:?}"))?;

    // The builder drains the static discovery channel into the peer cache.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut ticker = tokio::time::interval(Duration::from_millis(10));
    let found = loop {
        ticker.tick().await;
        if handle
            .cache()
            .list()
            .iter()
            .any(|p| p.name == "static-peer")
        {
            break true;
        }
        if tokio::time::Instant::now() >= deadline {
            break false;
        }
    };
    assert!(
        found,
        "a statically discovered peer must reach the peer cache"
    );

    handle.shutdown().await;
    Ok(())
}
