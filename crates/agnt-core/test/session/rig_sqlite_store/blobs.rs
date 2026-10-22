use super::*;

/// An image is saved as a blob named by its content and read back.
#[test]
fn images_round_trip_as_blobs() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-blobs")?;
    let bytes = b"hi";
    let message = image_message(bytes);
    let name = format!("{}.png", hex(Sha256::digest(bytes).as_slice()));

    // -- Exec: the saved message names the blob instead of holding the data
    let saved = store_images(&message, &store)?;

    // -- Check: the saved image names the blob
    let saved_image = saved.images().next().ok_or("the image is there")?;
    match &saved_image.data {
        DocumentSourceKind::Url(url) => assert_eq!(url, &format!("blob:{name}")),
        other => return Err(format!("expected a blob url, got {other:?}").into()),
    }

    // -- Check: the blob file is there, named by the content
    assert_eq!(store.read_file(&format!("blobs/{name}"))?, bytes);
    assert!(
        store.files()?.contains(&format!("blobs/{name}")),
        "the blob is one of the session's files"
    );

    // -- Exec: reading the message puts the data back
    let mut restored = saved.into_owned();
    load_images(&mut restored, &store)?;

    // -- Check: the image holds the data again
    let restored_image = restored.images().next().ok_or("the image is there")?;
    match &restored_image.data {
        DocumentSourceKind::Base64(data) => assert_eq!(data, "aGk="),
        other => return Err(format!("expected base64 data, got {other:?}").into()),
    }
    Ok(())
}
