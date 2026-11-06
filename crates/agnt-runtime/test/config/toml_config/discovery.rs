//! Config-file discovery: the XDG default and the `NU_AGENT_CONFIG_HOME` redirect.

use super::*;

/// The config file path the bootstrap follows: `{XDG_CONFIG_HOME}/{APP_DIR_NAME}/config.toml`.
#[test]
fn config_path_returns_xdg_path() -> Result<()> {
    with_xdg_config_home(|dir| {
        let expected = dir
            .path()
            .join(crate::utils::paths::APP_DIR_NAME)
            .join("config.toml");
        let path = crate::config::Paths::bootstrap_config_path_with(&env_for(dir))
            .map_err(|e| format!("bootstrap_config_path should succeed: {e:?}"))?;
        assert_eq!(path, expected);
        Ok(())
    })
}

/// `NU_AGENT_CONFIG_HOME` re-roots the config file path.
#[test]
fn config_path_honors_nu_agent_config_home() -> Result<()> {
    with_xdg_config_home(|dir| {
        // -- Setup & Fixtures
        let mut env = env_for(dir);
        let config_home = dir.path().join("elsewhere");
        env.insert(
            crate::config::ENV_CONFIG_HOME.to_string(),
            config_home.to_string_lossy().to_string(),
        );
        let redirected = config_home
            .join(crate::utils::paths::APP_DIR_NAME)
            .join("config.toml");

        // -- Exec
        let config_path = crate::config::Paths::bootstrap_config_path_with(&env)
            .map_err(|e| format!("bootstrap should succeed: {e:?}"))?;

        // -- Check: the env override wins over the XDG default.
        assert_eq!(config_path, redirected);
        Ok(())
    })
}
