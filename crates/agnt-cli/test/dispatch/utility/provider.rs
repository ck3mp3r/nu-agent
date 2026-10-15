use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_utility_provider_format_expiry() {
    // -- Exec & Check
    assert_eq!(format_expiry(None), "-");
    assert_eq!(format_expiry(Some(0)), "1970-01-01T00:00:00+00:00");
    assert_eq!(
        format_expiry(Some(1_700_000_000)),
        "2023-11-14T22:13:20+00:00"
    );
}

#[test]
fn test_utility_provider_remove_copilot_token_files() -> Result<()> {
    // -- Setup & Fixtures
    let dir = std::env::temp_dir().join(format!("agnt-cli-copilot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("access-token"), "tok")?;
    std::fs::write(dir.join("api-key.json"), "{}")?;

    // -- Exec
    remove_copilot_token_files(&dir)?;

    // -- Check
    assert!(!dir.join("access-token").exists());
    assert!(!dir.join("api-key.json").exists());

    // -- Exec & Check (idempotent when the files are already gone)
    remove_copilot_token_files(&dir)?;

    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
