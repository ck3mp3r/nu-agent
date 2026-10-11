use super::*;

#[tokio::test]
async fn test_file_upload_and_download_roundtrip() -> Result<()> {
    let (server, client) = test_server().await;

    // Upload a file
    let file_content = b"hello, a2a file exchange!";
    let upload_resp = client
        .post(format!("{}/files:upload", server.local_url))
        .body(file_content.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(upload_resp.status(), 200);

    let upload_body: serde_json::Value = upload_resp.json().await.unwrap();
    let file_id = upload_body["id"]
        .as_str()
        .ok_or("should have file id")?
        .to_string();
    assert!(!file_id.is_empty(), "file ID should not be empty");

    let file_url = upload_body["url"]
        .as_str()
        .ok_or("should have file url")?
        .to_string();
    assert!(
        file_url.contains(&file_id),
        "URL should contain the file ID"
    );

    // Download the file
    let download_resp = client
        .get(format!("{}/files/{}", server.local_url, file_id))
        .send()
        .await
        .unwrap();
    assert_eq!(download_resp.status(), 200);

    let downloaded = download_resp.bytes().await.unwrap();
    assert_eq!(
        downloaded.as_ref(),
        file_content,
        "downloaded content should match uploaded content"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_file_download_not_found() {
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/files/nonexistent-id", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "non-existent file should return 404");

    server.shutdown().await;
}

#[tokio::test]
async fn test_file_upload_multiple_files() -> Result<()> {
    let (server, client) = test_server().await;

    // Upload two files
    let resp1 = client
        .post(format!("{}/files:upload", server.local_url))
        .body(b"file one content".to_vec())
        .send()
        .await
        .unwrap();
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let id1 = body1["id"]
        .as_str()
        .ok_or("should have file id")?
        .to_string();

    let resp2 = client
        .post(format!("{}/files:upload", server.local_url))
        .body(b"file two content".to_vec())
        .send()
        .await
        .unwrap();
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let id2 = body2["id"]
        .as_str()
        .ok_or("should have file id")?
        .to_string();

    assert_ne!(id1, id2, "each upload should get a unique ID");

    // Download and verify both
    let dl1 = client
        .get(format!("{}/files/{}", server.local_url, id1))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(dl1.as_ref(), b"file one content");

    let dl2 = client
        .get(format!("{}/files/{}", server.local_url, id2))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(dl2.as_ref(), b"file two content");

    server.shutdown().await;
    Ok(())
}
