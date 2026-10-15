use super::*;

#[tokio::test]
async fn test_get_agent_card() -> Result<()> {
    let (_server, client, url) = test_setup().await;
    let card = get_agent_card(&client, &url)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(card.name, "test-server");
    assert_eq!(card.version, "1.0");
    Ok(())
}

#[tokio::test]
async fn test_get_agent_card_connection_refused() {
    let client = A2aClient::new().unwrap();
    let result = get_agent_card(&client, "http://127.0.0.1:1").await;
    assert!(matches!(result, Err(A2aError::ConnectionRefused(_))));
}
