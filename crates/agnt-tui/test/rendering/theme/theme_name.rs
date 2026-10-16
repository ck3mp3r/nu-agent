use super::theme_support_test::Result;
use crate::rendering::theme::{ThemeName, TuiTheme};

#[test]
fn theme_name_resolves_to_constructor() {
    assert_eq!(
        ThemeName::CatppuccinMocha.resolve(),
        TuiTheme::catppuccin_mocha()
    );
    assert_eq!(
        ThemeName::CatppuccinLatte.resolve(),
        TuiTheme::catppuccin_latte()
    );
    assert_eq!(
        ThemeName::CatppuccinFrappe.resolve(),
        TuiTheme::catppuccin_frappe()
    );
    assert_eq!(
        ThemeName::CatppuccinMacchiato.resolve(),
        TuiTheme::catppuccin_macchiato()
    );
}

#[test]
fn theme_name_all_returns_four_flavors() {
    assert_eq!(
        ThemeName::all(),
        [
            ThemeName::CatppuccinMocha,
            ThemeName::CatppuccinLatte,
            ThemeName::CatppuccinFrappe,
            ThemeName::CatppuccinMacchiato,
        ]
    );
}

#[test]
fn theme_name_from_name_accepts_new_flavors() {
    assert_eq!(
        ThemeName::from_name("catppuccin-frappe"),
        Some(ThemeName::CatppuccinFrappe)
    );
    assert_eq!(
        ThemeName::from_name("catppuccin-macchiato"),
        Some(ThemeName::CatppuccinMacchiato)
    );
}

#[test]
fn theme_name_kebab_name_returns_canonical_kebab_case() {
    assert_eq!(ThemeName::CatppuccinMocha.kebab_name(), "catppuccin-mocha");
    assert_eq!(ThemeName::CatppuccinLatte.kebab_name(), "catppuccin-latte");
    assert_eq!(
        ThemeName::CatppuccinFrappe.kebab_name(),
        "catppuccin-frappe"
    );
    assert_eq!(
        ThemeName::CatppuccinMacchiato.kebab_name(),
        "catppuccin-macchiato"
    );
}

#[test]
fn theme_name_kebab_name_round_trips_through_from_name() {
    for name in ThemeName::all() {
        let kebab = name.kebab_name();
        assert_eq!(
            ThemeName::from_name(kebab),
            Some(name),
            "kebab name {kebab} must resolve back to {name:?}"
        );
    }
}

#[test]
fn theme_pref_kebab_value_resolves_after_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("theme.json");
    let pref = agnt_core::theme_pref::ThemePreference {
        theme: Some("catppuccin-frappe".to_string()),
    };

    // -- Exec
    pref.save_to(&path)?;
    let loaded = agnt_core::theme_pref::ThemePreference::load_from(&path)?;
    let name = loaded
        .theme
        .as_deref()
        .ok_or("theme should be present after round-trip")?;

    // -- Check: the persisted kebab name still resolves to a ThemeName.
    assert_eq!(
        ThemeName::from_name(name),
        Some(ThemeName::CatppuccinFrappe)
    );
    Ok(())
}
