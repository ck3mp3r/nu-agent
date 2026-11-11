//! `theme` key parsing.

use super::*;

#[test]
fn load_parses_theme_key() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(dir, "theme = \"catppuccin-latte\"")?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config.theme.as_deref(), Some("catppuccin-latte"));
        Ok(())
    })
}

#[test]
fn load_keeps_invalid_theme_raw() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(dir, "theme = \"bogus\"")?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config.theme.as_deref(), Some("bogus"));
        Ok(())
    })
}
