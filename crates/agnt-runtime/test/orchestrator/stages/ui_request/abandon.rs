use super::*;

// ---------------------------------------------------------------------------
// abandon_unanswered
// ---------------------------------------------------------------------------

/// The concurrent lane is the second way a quit can wait forever. Abandoning
/// releases every entry that never answered.
#[tokio::test]
async fn abandon_unanswered_releases_the_concurrent_slots() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx).ok_or("the toggle should reach the worker")?;
    assert!(p.stage.has_pending(), "the dispatched toggle is open");

    // -- Exec
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.abandon_unanswered(&mut ctx).await;

    // -- Check
    assert!(!p.stage.has_pending(), "no abandoned toggle stays pending");
    Ok(())
}

/// An abandoned blocking slot frees the queue: the switch that waited behind it
/// reaches the worker on the next drain.
#[tokio::test]
async fn abandon_unanswered_frees_the_queued_switch() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "wedged-agent".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx).ok_or("the agent switch should reach the worker")?;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "queued-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    assert!(
        recv_command(p.worker_rx).is_none(),
        "the switch queues behind the open request"
    );

    // -- Exec
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.abandon_unanswered(&mut ctx).await;
    p.stage.drain_queued(&mut ctx).await;

    // -- Check
    match recv_command(p.worker_rx) {
        Some(WorkerCommand::HandleUiRequest {
            request: UiRequest::SwitchModel { spec },
            ..
        }) => assert_eq!(spec, "queued-model", "the freed queue drains in order"),
        _ => return Err("the queued switch should reach the worker once the slot is free".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The substitute answer an abandoned request gets
// ---------------------------------------------------------------------------

/// The words an abandoned request is answered with, frozen: they are the only
/// thing a surface is told about a response that never arrived.
fn abandonment(kind: &str) -> String {
    format!("{kind} abandoned at quit: the response was not received")
}

/// The words the log carries for the same event, frozen: a settled quit did not
/// lose the response, it simply outran it.
const ABANDONED_LOG: &str = "its response had not arrived when quit settled";

/// Every record this process logged: its level and its message.
static CAPTURED: std::sync::Mutex<Vec<(log::Level, String)>> = std::sync::Mutex::new(Vec::new());

/// Keeps the level of every record, so the log wording and its level are both
/// assertions rather than hopes.
struct LevelLogger;

impl log::Log for LevelLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if let Ok(mut captured) = CAPTURED.lock() {
            captured.push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

/// Capture every level from here on.
fn capture_levels() {
    let _ = log::set_boxed_logger(Box::new(LevelLogger));
    log::set_max_level(log::LevelFilter::Trace);
}

/// The records whose message contains `fragment`.
fn captured_matching(fragment: &str) -> Vec<(log::Level, String)> {
    CAPTURED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(_, message)| message.contains(fragment))
        .cloned()
        .collect()
}

/// WHEN quit abandons a blocking request, THEN that request's own lane SHALL
/// receive the substitute answer for its kind, worded the same for every kind.
#[tokio::test]
async fn abandon_answers_a_blocking_request_on_its_own_lane() -> Result<()> {
    // -- Setup & Fixtures
    let rows = [
        (
            "model switch",
            UiRequest::SwitchModel {
                spec: "wedged-model".to_string(),
            },
        ),
        (
            "agent switch",
            UiRequest::SwitchAgent {
                name: "wedged-agent".to_string(),
            },
        ),
        (
            "session switch",
            UiRequest::SwitchSession {
                id: "wedged-session".to_string(),
            },
        ),
    ];

    for (kind, request) in rows {
        let mut h = Harness::new();
        capture_levels();
        let p = h.parts();
        let mut ctx = make_ctx(
            p.worker_tx,
            p.blocking_tx,
            p.concurrent_tx,
            p.bus,
            p.ctx_state,
        );
        p.stage.handle_incoming(request, &mut ctx).await;
        assert!(
            recv_command(p.worker_rx).is_some(),
            "the {kind} request reaches the worker"
        );

        // -- Exec
        let mut ctx = make_ctx(
            p.worker_tx,
            p.blocking_tx,
            p.concurrent_tx,
            p.bus,
            p.ctx_state,
        );
        p.stage.abandon_unanswered(&mut ctx).await;

        // -- Check
        let answer = recv_blocking(p.blocking_rx)
            .ok_or_else(|| format!("an abandoned {kind} must be answered"))?;
        let text = match answer {
            UiRequestResponse::ModelSwitch(Err(text)) => text,
            UiRequestResponse::AgentSwitch(Err(text)) => text,
            UiRequestResponse::SessionSwitch {
                id,
                result: Err(text),
            } => {
                assert_eq!(id, "wedged-session", "the answer keeps the request's id");
                text
            }
            other => {
                return Err(
                    format!("the {kind} substitute must be its own kind: {other:?}").into(),
                );
            }
        };
        assert_eq!(text, abandonment(kind), "the notice wording is frozen");
        assert!(!p.stage.has_pending(), "an answered request is not pending");
        let logged = captured_matching(&format!("unanswered {kind} on quit: {ABANDONED_LOG}"));
        assert_eq!(
            logged.len(),
            1,
            "the {kind} abandonment logs the settled wording once: {logged:?}"
        );
        assert_eq!(
            logged[0].0,
            log::Level::Warn,
            "a quit that outran a response is a warning"
        );
    }
    Ok(())
}

/// WHEN quit abandons a toggle, THEN the concurrent lane SHALL receive the
/// toggle's substitute carrying the tool-count facts already published, so
/// folding it leaves the footer on its last-known numbers.
#[tokio::test]
async fn abandon_answers_a_toggle_with_the_last_known_facts() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    // An answered toggle first: its facts are what the surface shows now.
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "alpha".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx).ok_or("the first toggle reaches the worker")?;
    p.stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "alpha".to_string(),
                result: Ok(McpUsabilityState::Enabled),
                total: 7,
                server_count: 2,
                names_by_server: vec![(
                    "alpha".to_string(),
                    vec!["alpha__one".to_string(), "alpha__two".to_string()],
                )],
            },
            &mut ctx,
        )
        .await;
    let _ = take_facts(p.facts_rx);
    // A second toggle the worker never answers.
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "beta".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx).ok_or("the second toggle reaches the worker")?;

    // -- Exec
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.abandon_unanswered(&mut ctx).await;

    // -- Check
    let answer = recv_concurrent(p.concurrent_rx)
        .ok_or("an abandoned toggle must be answered on the concurrent lane")?;
    let UiRequestResponse::McpToggle {
        server,
        result: Err(text),
        total,
        server_count,
        names_by_server,
    } = answer
    else {
        return Err(format!("the toggle substitute must be a toggle: {answer:?}").into());
    };
    assert_eq!(server, "beta");
    assert_eq!(
        text,
        abandonment("MCP toggle"),
        "the notice wording is frozen"
    );
    assert_eq!(total, 7, "the substitute keeps the published total");
    assert_eq!(
        server_count, 0,
        "a server with no published count keeps the zero the footer shows"
    );
    assert_eq!(
        names_by_server,
        vec![(
            "alpha".to_string(),
            vec!["alpha__one".to_string(), "alpha__two".to_string()]
        )],
        "the substitute republishes the names already on the surface"
    );
    assert!(!p.stage.has_pending(), "an answered toggle is not pending");

    // -- Exec: folding the substitute is what the loop does on quit.
    p.stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server,
                result: Err(text),
                total,
                server_count,
                names_by_server,
            },
            &mut ctx,
        )
        .await;

    // -- Check: the footer keeps its numbers and the server reads as failed.
    let facts = take_facts(p.facts_rx);
    assert!(
        facts.contains(&FactRecord::McpVisibleToolTotal(7)),
        "the total does not move: {facts:?}"
    );
    assert!(
        facts.contains(&FactRecord::McpVisibleToolNames {
            server: "alpha".to_string(),
            names: vec!["alpha__one".to_string(), "alpha__two".to_string()],
        }),
        "the published names do not move: {facts:?}"
    );
    assert!(
        facts.contains(&FactRecord::McpServerState {
            server: "beta".to_string(),
            state: McpServerUsability::Failed,
            error: Some(abandonment("MCP toggle")),
        }),
        "the abandoned toggle releases as failed, with the frozen notice: {facts:?}"
    );
    Ok(())
}
// ---------------------------------------------------------------------------
// A blocking response releases the queue
// ---------------------------------------------------------------------------

/// A switch queued behind an open blocking request applies when that request's
/// response lands: the response clears the slot and the drain dispatches the
/// switch, with no second apply path.
#[tokio::test]
async fn blocking_response_drain_applies_the_queued_switch() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "open-agent".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx).ok_or("the agent switch should reach the worker")?;

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "queued-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    assert!(
        recv_command(p.worker_rx).is_none(),
        "the switch queues behind the open request"
    );
    assert!(
        !take_warnings(p.narration_rx).is_empty(),
        "the deferral is announced where it lands"
    );

    // -- Exec
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::AgentSwitch(Err("switch failed".to_string())),
            &mut ctx,
        )
        .await;
    p.stage.drain_queued(&mut ctx).await;

    // -- Check
    match recv_command(p.worker_rx) {
        Some(WorkerCommand::HandleUiRequest {
            request: UiRequest::SwitchModel { spec },
            ..
        }) => assert_eq!(spec, "queued-model", "the queued switch applies now"),
        _ => return Err("the blocking response should drain the queued switch".into()),
    }
    Ok(())
}
