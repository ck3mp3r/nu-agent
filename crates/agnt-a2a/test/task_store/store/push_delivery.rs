use std::time::Duration;

use super::*;

// ---------------------------------------------------------------------------
// Record-and-reply webhook stub
// ---------------------------------------------------------------------------

/// One delivery as the stub received it.
#[derive(Clone, Debug)]
struct RecordedPush {
    method: String,
    a2a_version: Option<String>,
    body: serde_json::Value,
}

/// Start a localhost webhook that records every delivery it receives.
async fn start_recording_webhook() -> Result<(String, tokio::sync::mpsc::Receiver<RecordedPush>)> {
    let (tx, rx) = tokio::sync::mpsc::channel::<RecordedPush>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let app = axum::Router::new().route(
        "/hook",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::Json<serde_json::Value>| {
                let tx = tx.clone();
                async move {
                    let _ = tx
                        .send(RecordedPush {
                            method: "POST".to_string(),
                            a2a_version: headers
                                .get("a2a-version")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                            body: body.0,
                        })
                        .await;
                    axum::Json(serde_json::json!({ "ok": true }))
                }
            },
        ),
    );

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((format!("http://127.0.0.1:{}/hook", addr.port()), rx))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_push_delivery_posts_the_stream_payload_with_a2a_version_header() -> Result<()> {
    // -- Setup & Fixtures
    let (url, mut deliveries) = start_recording_webhook().await?;
    let store = InMemoryTaskStore::with_transport(crate::test_support::test_transport());
    let task = store.create_task(Some("ctx-push".to_string()), None, None);

    // Reach a non-terminal state first so the config exists before the
    // terminal transition that must fire exactly one delivery.
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("the working transition should succeed: {e:?}"))?;
    store.create_push_config(&task.id, &url, None);

    // -- Exec
    store
        .update_status(&task.id, TaskState::Completed, None)
        .map_err(|e| format!("the terminal transition should succeed: {e:?}"))?;

    // -- Check
    let delivery = tokio::time::timeout(Duration::from_secs(5), deliveries.recv())
        .await
        .map_err(|_| "the push delivery should arrive within 5 s")?
        .ok_or("the webhook stub should record a delivery")?;

    assert_eq!(delivery.method, "POST");
    assert_eq!(
        delivery.a2a_version.as_deref(),
        Some(crate::A2A_VERSION),
        "a push delivery must carry the transport's a2a-version header"
    );

    let update = delivery
        .body
        .get("statusUpdate")
        .ok_or("the delivery body must be a StreamResponse statusUpdate")?;
    assert_eq!(update["taskId"].as_str(), Some(task.id.as_str()));
    assert_eq!(update["contextId"].as_str(), Some("ctx-push"));
    assert_eq!(
        update["status"]["state"].as_str(),
        Some("TASK_STATE_COMPLETED")
    );

    let duplicate = tokio::time::timeout(Duration::from_millis(200), deliveries.recv()).await;
    assert!(
        duplicate.is_err(),
        "one status transition must produce exactly one delivery"
    );
    Ok(())
}
