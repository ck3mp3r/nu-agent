use super::InMemoryTaskStore;
use crate::{Artifact, Part, TaskEvent, TaskState};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_subscribe_receives_status_update() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let (mut rx, _) = store.subscribe(&task.id);

    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;
    assert!(event.is_ok(), "Should receive status update");
    match event.unwrap().unwrap() {
        TaskEvent::StatusChanged { status, .. } => {
            assert_eq!(status.state, TaskState::Working);
        }
        _ => panic!("expected StatusChanged"),
    }
    Ok(())
}

#[tokio::test]
async fn test_subscribe_receives_artifact_added() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let (mut rx, _) = store.subscribe(&task.id);

    let artifact = Artifact {
        artifact_id: "art-1".to_string(),
        name: Some("result".to_string()),
        parts: vec![Part::Text {
            text: "output".into(),
        }],
        metadata: None,
    };
    store
        .add_artifact(&task.id, artifact.clone())
        .map_err(|e| format!("{e:?}"))?;

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;
    assert!(event.is_ok(), "Should receive artifact added event");
    match event.unwrap().unwrap() {
        TaskEvent::ArtifactAdded { artifact: a, .. } => {
            assert_eq!(a.artifact_id, "art-1");
            assert_eq!(a.name, Some("result".to_string()));
        }
        _ => panic!("expected ArtifactAdded"),
    }
    Ok(())
}
