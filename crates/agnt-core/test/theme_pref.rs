use super::*;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A saved preference round-trips, is stored as a kebab-case JSON value, and
/// the file is written with mode 0600.
#[test]
fn theme_pref_round_trip_and_persistence() -> Result<()> {
    // -- Case 1: save then load round-trips the theme name
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    let pref = ThemePreference {
        theme: Some("catppuccin-frappe".to_string()),
    };
    pref.save_to(&path)?;
    let loaded = ThemePreference::load_from(&path)?;
    assert_eq!(loaded.theme.as_deref(), Some("catppuccin-frappe"));

    // -- Case 2: the file stores the kebab-case name, not a Debug-format name
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    let pref = ThemePreference {
        theme: Some("catppuccin-macchiato".to_string()),
    };
    pref.save_to(&path)?;
    let raw = std::fs::read_to_string(&path)?;
    assert!(
        raw.contains("\"catppuccin-macchiato\""),
        "theme.json must contain the kebab-case name, got: {raw}"
    );
    assert!(
        !raw.contains("CatppuccinMacchiato"),
        "theme.json must not contain a Debug-format name, got: {raw}"
    );

    // -- Case 3: the file is written with mode 0600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
        let path = dir.path().join("theme.json");
        let pref = ThemePreference {
            theme: Some("catppuccin-latte".to_string()),
        };
        pref.save_to(&path)?;
        let mode = std::fs::metadata(&path)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    Ok(())
}

/// A missing or corrupt preference file loads as the default.
#[test]
fn theme_pref_defaults() -> Result<()> {
    // -- Case 1: a missing file returns the default
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    assert_eq!(ThemePreference::load_from(&path)?.theme, None);

    // -- Case 2: a corrupt file returns the default
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    std::fs::write(&path, "not valid json")?;
    assert_eq!(ThemePreference::load_from(&path)?.theme, None);
    Ok(())
}
