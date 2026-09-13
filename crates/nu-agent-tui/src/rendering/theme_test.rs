use ratatui::style::{Color, Modifier};

use crate::rendering::theme::{CatppuccinPalette, ThemeName, TuiTheme};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const MOCHA_RED: Color = Color::Rgb(243, 139, 168);
const MOCHA_YELLOW: Color = Color::Rgb(249, 226, 175);
const MOCHA_GREEN: Color = Color::Rgb(166, 227, 161);
const MOCHA_BLUE: Color = Color::Rgb(137, 180, 250);
const MOCHA_SAPPHIRE: Color = Color::Rgb(116, 199, 236);
const MOCHA_LAVENDER: Color = Color::Rgb(180, 190, 254);
const MOCHA_MAUVE: Color = Color::Rgb(203, 166, 247);
const MOCHA_PEACH: Color = Color::Rgb(250, 179, 135);
const MOCHA_OVERLAY0: Color = Color::Rgb(108, 112, 134);
const MOCHA_OVERLAY1: Color = Color::Rgb(127, 132, 156);

#[test]
fn mocha_palette_matches_official_values() {
    let p = CatppuccinPalette::mocha();

    assert_eq!(p.rosewater, Color::Rgb(245, 224, 220));
    assert_eq!(p.flamingo, Color::Rgb(242, 205, 205));
    assert_eq!(p.pink, Color::Rgb(245, 194, 231));
    assert_eq!(p.mauve, Color::Rgb(203, 166, 247));
    assert_eq!(p.red, Color::Rgb(243, 139, 168));
    assert_eq!(p.maroon, Color::Rgb(235, 160, 172));
    assert_eq!(p.peach, Color::Rgb(250, 179, 135));
    assert_eq!(p.yellow, Color::Rgb(249, 226, 175));
    assert_eq!(p.green, Color::Rgb(166, 227, 161));
    assert_eq!(p.teal, Color::Rgb(148, 226, 213));
    assert_eq!(p.sky, Color::Rgb(137, 220, 235));
    assert_eq!(p.sapphire, Color::Rgb(116, 199, 236));
    assert_eq!(p.blue, Color::Rgb(137, 180, 250));
    assert_eq!(p.lavender, Color::Rgb(180, 190, 254));
    assert_eq!(p.text, Color::Rgb(205, 214, 244));
    assert_eq!(p.subtext0, Color::Rgb(166, 173, 200));
    assert_eq!(p.subtext1, Color::Rgb(186, 194, 222));
    assert_eq!(p.overlay0, Color::Rgb(108, 112, 134));
    assert_eq!(p.overlay1, Color::Rgb(127, 132, 156));
    assert_eq!(p.overlay2, Color::Rgb(147, 153, 178));
    assert_eq!(p.surface0, Color::Rgb(49, 50, 68));
    assert_eq!(p.surface1, Color::Rgb(69, 71, 90));
    assert_eq!(p.surface2, Color::Rgb(88, 91, 112));
    assert_eq!(p.base, Color::Rgb(30, 30, 46));
    assert_eq!(p.mantle, Color::Rgb(24, 24, 37));
    assert_eq!(p.crust, Color::Rgb(17, 17, 27));
}

#[test]
fn latte_palette_matches_official_values() {
    let p = CatppuccinPalette::latte();

    assert_eq!(p.rosewater, Color::Rgb(220, 138, 120));
    assert_eq!(p.flamingo, Color::Rgb(221, 120, 120));
    assert_eq!(p.pink, Color::Rgb(234, 118, 203));
    assert_eq!(p.mauve, Color::Rgb(136, 57, 239));
    assert_eq!(p.red, Color::Rgb(210, 15, 57));
    assert_eq!(p.maroon, Color::Rgb(230, 69, 83));
    assert_eq!(p.peach, Color::Rgb(254, 100, 11));
    assert_eq!(p.yellow, Color::Rgb(223, 142, 29));
    assert_eq!(p.green, Color::Rgb(64, 160, 43));
    assert_eq!(p.teal, Color::Rgb(23, 146, 153));
    assert_eq!(p.sky, Color::Rgb(4, 165, 229));
    assert_eq!(p.sapphire, Color::Rgb(32, 159, 181));
    assert_eq!(p.blue, Color::Rgb(30, 102, 245));
    assert_eq!(p.lavender, Color::Rgb(114, 135, 253));
    assert_eq!(p.text, Color::Rgb(76, 79, 105));
    assert_eq!(p.subtext0, Color::Rgb(108, 111, 133));
    assert_eq!(p.subtext1, Color::Rgb(92, 95, 119));
    assert_eq!(p.overlay0, Color::Rgb(156, 160, 176));
    assert_eq!(p.overlay1, Color::Rgb(140, 143, 161));
    assert_eq!(p.overlay2, Color::Rgb(124, 127, 147));
    assert_eq!(p.surface0, Color::Rgb(204, 208, 218));
    assert_eq!(p.surface1, Color::Rgb(188, 192, 204));
    assert_eq!(p.surface2, Color::Rgb(172, 176, 190));
    assert_eq!(p.base, Color::Rgb(239, 241, 245));
    assert_eq!(p.mantle, Color::Rgb(230, 233, 239));
    assert_eq!(p.crust, Color::Rgb(220, 224, 232));
}

#[test]
fn frappe_palette_matches_official_values() {
    let p = CatppuccinPalette::frappe();

    assert_eq!(p.rosewater, Color::Rgb(242, 213, 207));
    assert_eq!(p.flamingo, Color::Rgb(238, 190, 190));
    assert_eq!(p.pink, Color::Rgb(244, 184, 228));
    assert_eq!(p.mauve, Color::Rgb(202, 158, 230));
    assert_eq!(p.red, Color::Rgb(231, 130, 132));
    assert_eq!(p.maroon, Color::Rgb(234, 153, 156));
    assert_eq!(p.peach, Color::Rgb(239, 159, 118));
    assert_eq!(p.yellow, Color::Rgb(229, 200, 144));
    assert_eq!(p.green, Color::Rgb(166, 209, 137));
    assert_eq!(p.teal, Color::Rgb(129, 200, 190));
    assert_eq!(p.sky, Color::Rgb(153, 209, 219));
    assert_eq!(p.sapphire, Color::Rgb(133, 193, 220));
    assert_eq!(p.blue, Color::Rgb(140, 170, 238));
    assert_eq!(p.lavender, Color::Rgb(186, 187, 241));
    assert_eq!(p.text, Color::Rgb(198, 208, 245));
    assert_eq!(p.subtext0, Color::Rgb(165, 173, 206));
    assert_eq!(p.subtext1, Color::Rgb(181, 191, 226));
    assert_eq!(p.overlay0, Color::Rgb(115, 121, 148));
    assert_eq!(p.overlay1, Color::Rgb(131, 139, 167));
    assert_eq!(p.overlay2, Color::Rgb(148, 156, 187));
    assert_eq!(p.surface0, Color::Rgb(65, 69, 89));
    assert_eq!(p.surface1, Color::Rgb(81, 87, 109));
    assert_eq!(p.surface2, Color::Rgb(98, 104, 128));
    assert_eq!(p.base, Color::Rgb(48, 52, 70));
    assert_eq!(p.mantle, Color::Rgb(41, 44, 60));
    assert_eq!(p.crust, Color::Rgb(35, 38, 52));
}

#[test]
fn macchiato_palette_matches_official_values() {
    let p = CatppuccinPalette::macchiato();

    assert_eq!(p.rosewater, Color::Rgb(244, 219, 214));
    assert_eq!(p.flamingo, Color::Rgb(240, 198, 198));
    assert_eq!(p.pink, Color::Rgb(245, 189, 230));
    assert_eq!(p.mauve, Color::Rgb(198, 160, 246));
    assert_eq!(p.red, Color::Rgb(237, 135, 150));
    assert_eq!(p.maroon, Color::Rgb(238, 153, 160));
    assert_eq!(p.peach, Color::Rgb(245, 169, 127));
    assert_eq!(p.yellow, Color::Rgb(238, 212, 159));
    assert_eq!(p.green, Color::Rgb(166, 218, 149));
    assert_eq!(p.teal, Color::Rgb(139, 213, 202));
    assert_eq!(p.sky, Color::Rgb(145, 215, 227));
    assert_eq!(p.sapphire, Color::Rgb(125, 196, 228));
    assert_eq!(p.blue, Color::Rgb(138, 173, 244));
    assert_eq!(p.lavender, Color::Rgb(183, 189, 248));
    assert_eq!(p.text, Color::Rgb(202, 211, 245));
    assert_eq!(p.subtext0, Color::Rgb(165, 173, 203));
    assert_eq!(p.subtext1, Color::Rgb(184, 192, 224));
    assert_eq!(p.overlay0, Color::Rgb(110, 115, 141));
    assert_eq!(p.overlay1, Color::Rgb(128, 135, 162));
    assert_eq!(p.overlay2, Color::Rgb(147, 154, 183));
    assert_eq!(p.surface0, Color::Rgb(54, 58, 79));
    assert_eq!(p.surface1, Color::Rgb(73, 77, 100));
    assert_eq!(p.surface2, Color::Rgb(91, 96, 120));
    assert_eq!(p.base, Color::Rgb(36, 39, 58));
    assert_eq!(p.mantle, Color::Rgb(30, 32, 48));
    assert_eq!(p.crust, Color::Rgb(24, 25, 38));
}

#[test]
fn syntax_channels_map_to_catppuccin_mocha_colors_and_styles() {
    let theme = TuiTheme::default();

    assert_eq!(theme.syntax_keyword.fg, Some(MOCHA_MAUVE));
    assert_eq!(theme.syntax_type.fg, Some(MOCHA_YELLOW));
    assert_eq!(theme.syntax_function.fg, Some(MOCHA_BLUE));
    assert_eq!(theme.syntax_variable.fg, Some(MOCHA_LAVENDER));
    assert_eq!(theme.syntax_constant.fg, Some(MOCHA_RED));
    assert_eq!(theme.syntax_string.fg, Some(MOCHA_GREEN));
    assert_eq!(theme.syntax_number.fg, Some(MOCHA_PEACH));
    assert_eq!(theme.syntax_operator.fg, Some(MOCHA_SAPPHIRE));
    assert_eq!(theme.syntax_punctuation.fg, Some(MOCHA_OVERLAY1));
    assert_eq!(theme.syntax_comment.fg, Some(MOCHA_OVERLAY0));
    assert!(theme.syntax_comment.add_modifier.contains(Modifier::DIM));
}

#[test]
fn existing_role_and_status_channels_remain_unchanged() {
    let theme = TuiTheme::default();

    assert_eq!(theme.role_user.fg, Some(MOCHA_LAVENDER));
    assert_eq!(theme.role_assistant.fg, Some(MOCHA_LAVENDER));
    assert_eq!(theme.role_system.fg, Some(MOCHA_YELLOW));
    assert_eq!(theme.role_compaction.fg, Some(MOCHA_OVERLAY1));
    assert_eq!(theme.role_tool.fg, Some(MOCHA_MAUVE));

    assert_eq!(theme.status_queued.fg, Some(MOCHA_OVERLAY0));
    assert_eq!(theme.status_running.fg, Some(MOCHA_SAPPHIRE));
    assert_eq!(theme.status_done.fg, Some(MOCHA_GREEN));
    assert_eq!(theme.status_failed.fg, Some(MOCHA_RED));
    assert_eq!(theme.status_cancelled.fg, Some(MOCHA_OVERLAY0));
    assert_eq!(theme.row_user_bg, theme.surface0);
}

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
    let pref = nu_agent_core::theme_pref::ThemePreference {
        theme: Some("catppuccin-frappe".to_string()),
    };

    // -- Exec
    pref.save_to(&path)?;
    let loaded = nu_agent_core::theme_pref::ThemePreference::load_from(&path)?;
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

#[test]
fn frappe_resolves_to_frappe_palette() {
    let theme = ThemeName::CatppuccinFrappe.resolve();

    assert_eq!(theme.syntax_keyword.fg, Some(Color::Rgb(202, 158, 230)));
    assert_eq!(theme.syntax_string.fg, Some(Color::Rgb(166, 209, 137)));
}

#[test]
fn macchiato_resolves_to_macchiato_palette() {
    let theme = ThemeName::CatppuccinMacchiato.resolve();

    assert_eq!(theme.syntax_keyword.fg, Some(Color::Rgb(198, 160, 246)));
    assert_eq!(theme.status_failed.fg, Some(Color::Rgb(237, 135, 150)));
}

#[test]
fn catppuccin_latte_maps_to_latte_palette() {
    let theme = TuiTheme::catppuccin_latte();

    assert_eq!(theme.syntax_keyword.fg, Some(Color::Rgb(136, 57, 239)));
    assert_eq!(theme.role_user.fg, Some(Color::Rgb(114, 135, 253)));
    assert_eq!(theme.role_assistant.fg, Some(Color::Rgb(114, 135, 253)));
    assert_eq!(theme.status_failed.fg, Some(Color::Rgb(210, 15, 57)));
    assert_eq!(theme.status_done.fg, Some(Color::Rgb(64, 160, 43)));
    assert_eq!(theme.row_user_bg, theme.surface0);
}

#[test]
fn mocha_surface0_feeds_code_block_background() {
    let theme = TuiTheme::catppuccin_mocha();

    assert_eq!(theme.surface0, Color::Rgb(49, 50, 68));
}

#[test]
fn base_matches_palette_base_for_both_flavors() {
    let mocha = TuiTheme::catppuccin_mocha();
    let latte = TuiTheme::catppuccin_latte();

    assert_eq!(mocha.base, CatppuccinPalette::mocha().base);
    assert_eq!(mocha.base, Color::Rgb(30, 30, 46));
    assert_eq!(latte.base, CatppuccinPalette::latte().base);
    assert_eq!(latte.base, Color::Rgb(239, 241, 245));
}

#[test]
fn row_user_bg_equals_surface0_for_both_flavors() {
    let mocha = TuiTheme::catppuccin_mocha();
    let latte = TuiTheme::catppuccin_latte();

    assert_eq!(mocha.row_user_bg, mocha.surface0);
    assert_eq!(latte.row_user_bg, latte.surface0);
}

#[test]
fn latte_body_text_uses_dark_text_fg() {
    let theme = TuiTheme::catppuccin_latte();
    let text = Color::Rgb(76, 79, 105);

    assert_eq!(theme.input_text.fg, Some(text));
    assert_eq!(theme.row_user.fg, Some(text));
    assert_eq!(theme.row_assistant.fg, Some(text));
    assert_eq!(theme.row_tool.fg, Some(text));
    assert_eq!(theme.row_system.fg, Some(text));
}
