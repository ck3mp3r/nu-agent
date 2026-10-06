use super::*;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn theme_pref_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    let pref = ThemePreference {
        theme: Some("catppuccin-frappe".to_string()),
    };

    // -- Exec
    pref.save_to(&path)?;
    let loaded = ThemePreference::load_from(&path)?;

    // -- Check
    assert_eq!(loaded.theme.as_deref(), Some("catppuccin-frappe"));
    Ok(())
}

#[test]
fn theme_pref_persists_kebab_case_json_value() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    let pref = ThemePreference {
        theme: Some("catppuccin-macchiato".to_string()),
    };

    // -- Exec
    pref.save_to(&path)?;
    let raw = std::fs::read_to_string(&path)?;

    // -- Check: the file stores the kebab-case name, not a Debug-format name.
    assert!(
        raw.contains("\"catppuccin-macchiato\""),
        "theme.json must contain the kebab-case name, got: {raw}"
    );
    assert!(
        !raw.contains("CatppuccinMacchiato"),
        "theme.json must not contain a Debug-format name, got: {raw}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn theme_pref_saves_with_mode_0600() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // -- Setup & Fixtures
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    let pref = ThemePreference {
        theme: Some("catppuccin-latte".to_string()),
    };

    // -- Exec
    pref.save_to(&path)?;
    let mode = std::fs::metadata(&path)?.permissions().mode() & 0o777;

    // -- Check
    assert_eq!(mode, 0o600);
    Ok(())
}

#[test]
fn theme_pref_missing_file_returns_default() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");

    // -- Exec
    let loaded = ThemePreference::load_from(&path)?;

    // -- Check
    assert_eq!(loaded.theme, None);
    Ok(())
}

#[test]
fn theme_pref_corrupt_file_returns_default() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    std::fs::write(&path, "not valid json")?;

    // -- Exec
    let loaded = ThemePreference::load_from(&path)?;

    // -- Check
    assert_eq!(loaded.theme, None);
    Ok(())
}
