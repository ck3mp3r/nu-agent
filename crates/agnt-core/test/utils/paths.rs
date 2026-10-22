//! Tests for the compile-time application directory name flip.
//!
//! Every test injects an explicit environment map, so no test mutates
//! process-global state and the whole module runs in parallel.

use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;
use crate::utils::paths::*;
use crate::utils::xdg::XdgError;
use std::path::PathBuf;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The directory name the current build must resolve.
#[cfg(debug_assertions)]
const EXPECTED_APP_DIR: &str = "nu-agent-dev";

/// The directory name the current build must resolve.
#[cfg(not(debug_assertions))]
const EXPECTED_APP_DIR: &str = "nu-agent";

// region:    --- Tests

#[test]
fn test_paths_app_dir_flips_name_on_debug_assertions() -> Result<()> {
    // -- Setup & Fixtures
    let base = PathBuf::from("/base");

    // -- Exec
    let resolved = app_dir(&base);

    // -- Check
    assert_eq!(
        APP_DIR_NAME, EXPECTED_APP_DIR,
        "the directory name flips at compile time"
    );
    assert_eq!(resolved, base.join(EXPECTED_APP_DIR));
    Ok(())
}

#[test]
fn test_paths_data_app_dir_honors_xdg_data_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("XDG_DATA_HOME", "/custom/data")]);

    // -- Exec
    let resolved = data_app_dir_with(&env).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        resolved,
        PathBuf::from("/custom/data").join(EXPECTED_APP_DIR)
    );
    Ok(())
}

#[test]
fn test_paths_data_app_dir_falls_back_to_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester")]);

    // -- Exec
    let resolved = data_app_dir_with(&env).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        resolved,
        PathBuf::from("/home/tester")
            .join(".local")
            .join("share")
            .join(EXPECTED_APP_DIR)
    );
    Ok(())
}

#[test]
fn test_paths_data_app_dir_reports_missing_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let err = match data_app_dir_with(&env) {
        Ok(_) => return Err("data_app_dir should fail when HOME is missing".into()),
        Err(e) => e,
    };

    // -- Check
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

// endregion: --- Tests
