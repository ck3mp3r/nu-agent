use super::*;

#[test]
fn test_create_and_list_push_config() {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", None);
    assert_eq!(config.url, "https://hook.example.com/notify");
    assert_eq!(config.task_id, task.id);
    assert!(!config.id.is_empty());

    let configs = store.list_push_configs(&task.id);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].url, "https://hook.example.com/notify");
}

#[test]
fn test_create_push_config_with_bearer_auth() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let auth = PushAuthenticationInfo {
        scheme: PushAuthScheme::Bearer {
            token: "sekret".to_string(),
        },
    };
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", Some(auth));
    assert!(config.authentication.is_some());
    let auth = config.authentication.ok_or("should have authentication")?;
    match &auth.scheme {
        PushAuthScheme::Bearer { token } => assert_eq!(token, "sekret"),
        _ => panic!("expected Bearer auth"),
    }
    Ok(())
}

#[test]
fn test_delete_push_config() {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", None);
    assert!(!store.list_push_configs(&task.id).is_empty());

    store.delete_push_config(&task.id, &config.id);
    assert!(store.list_push_configs(&task.id).is_empty());
}

#[test]
fn test_get_push_config() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", None);

    let found = store.get_push_config(&task.id, &config.id);
    assert!(found.is_some());
    let found = found.ok_or("should find push config")?;
    assert_eq!(found.id, config.id);

    let not_found = store.get_push_config(&task.id, "nonexistent");
    assert!(not_found.is_none());
    Ok(())
}
