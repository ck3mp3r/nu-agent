//! The standalone process-wide agent event channel.

use super::*;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_bus_agent_channel_round_trips_started_and_shutdown() -> Result<()> {
    // -- Setup & Fixtures
    let tx = AgentTx::new("agent", 16);
    let mut rx = tx.subscribe();

    // -- Exec
    tx.send(AgentEvent::Started {
        mode: "tui".to_string(),
    })
    .await
    .map_err(|e| format!("send should succeed: {e:?}"))?;
    tx.send(AgentEvent::Shutdown {
        reason: "completed".to_string(),
    })
    .await
    .map_err(|e| format!("send should succeed: {e:?}"))?;

    // -- Check
    let started = timeout(Duration::from_millis(100), rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    match started {
        AgentEvent::Started { mode } => assert_eq!(mode, "tui"),
        _ => return Err("expected AgentEvent::Started".into()),
    }
    let shutdown = timeout(Duration::from_millis(100), rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    match shutdown {
        AgentEvent::Shutdown { reason } => assert_eq!(reason, "completed"),
        _ => return Err("expected AgentEvent::Shutdown".into()),
    }
    Ok(())
}

#[tokio::test]
async fn test_bus_agent_channel_is_independent_of_the_bus() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let tx = AgentTx::new("agent", 16);
    let mut agent_rx = tx.subscribe();
    let mut turn_rx = bus.turn().subscribe();

    // -- Exec
    tx.send(AgentEvent::Started {
        mode: "acp".to_string(),
    })
    .await
    .map_err(|e| format!("send should succeed: {e:?}"))?;

    // -- Check
    let received = timeout(Duration::from_millis(100), agent_rx.recv())
        .await
        .map_err(|_| "the agent channel must deliver")?
        .map_err(|_| "receive should succeed")?;
    assert!(matches!(received, AgentEvent::Started { .. }));
    assert!(
        turn_rx.try_recv().is_err(),
        "an agent event must not reach a bus channel"
    );
    Ok(())
}
