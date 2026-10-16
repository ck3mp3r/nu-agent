use ratatui::style::{Color, Modifier};

use super::theme_support_test::{
    MOCHA_BLUE, MOCHA_GREEN, MOCHA_LAVENDER, MOCHA_MAUVE, MOCHA_OVERLAY0, MOCHA_OVERLAY1,
    MOCHA_OVERLAY2, MOCHA_PEACH, MOCHA_RED, MOCHA_SAPPHIRE, MOCHA_SKY, MOCHA_TEXT, MOCHA_YELLOW,
};
use crate::rendering::theme::{CatppuccinPalette, ThemeName, TuiTheme};

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
    assert_eq!(p.surface0, Color::Rgb(43, 44, 61));
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
    assert_eq!(p.surface0, Color::Rgb(196, 200, 212));
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
    assert_eq!(p.surface0, Color::Rgb(59, 63, 82));
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
    assert_eq!(p.surface0, Color::Rgb(48, 52, 72));
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
    assert_eq!(theme.syntax_variable.fg, Some(MOCHA_TEXT));
    assert_eq!(theme.syntax_constant.fg, Some(MOCHA_PEACH));
    assert_eq!(theme.syntax_string.fg, Some(MOCHA_GREEN));
    assert_eq!(theme.syntax_number.fg, Some(MOCHA_PEACH));
    assert_eq!(theme.syntax_operator.fg, Some(MOCHA_SKY));
    assert_eq!(theme.syntax_punctuation.fg, Some(MOCHA_OVERLAY2));
    assert_eq!(theme.syntax_comment.fg, Some(MOCHA_OVERLAY2));
    assert!(theme.syntax_comment.add_modifier.contains(Modifier::ITALIC));
    assert!(!theme.syntax_comment.add_modifier.contains(Modifier::DIM));
}

/// WHEN the theme is built from any palette, THE recolored syntax channels and
/// inline code SHALL follow the catppuccin/nvim spec: variable = text,
/// constant = peach, operator = sky, punctuation = overlay2, comment =
/// overlay2 + italic with no dim, and inline code = green with no dim.
#[test]
fn recolored_syntax_channels_follow_nvim_spec_for_all_flavors() {
    let cases = [
        (TuiTheme::catppuccin_mocha(), CatppuccinPalette::mocha()),
        (TuiTheme::catppuccin_latte(), CatppuccinPalette::latte()),
        (TuiTheme::catppuccin_frappe(), CatppuccinPalette::frappe()),
        (
            TuiTheme::catppuccin_macchiato(),
            CatppuccinPalette::macchiato(),
        ),
    ];

    for (theme, palette) in cases {
        assert_eq!(theme.syntax_variable.fg, Some(palette.text));
        assert_eq!(theme.syntax_constant.fg, Some(palette.peach));
        assert_eq!(theme.syntax_operator.fg, Some(palette.sky));
        assert_eq!(theme.syntax_punctuation.fg, Some(palette.overlay2));

        assert_eq!(theme.syntax_comment.fg, Some(palette.overlay2));
        assert!(
            theme.syntax_comment.add_modifier.contains(Modifier::ITALIC),
            "comment must be italic"
        );
        assert!(
            !theme.syntax_comment.add_modifier.contains(Modifier::DIM),
            "comment must not be dim"
        );

        assert_eq!(theme.inline_code.fg, Some(palette.green));
        assert!(
            !theme.inline_code.add_modifier.contains(Modifier::DIM),
            "inline code must not be dim"
        );
    }
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
    assert_eq!(theme.row_user_bg, Color::Rgb(43, 44, 61));
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
    assert_eq!(theme.row_user_bg, Color::Rgb(196, 200, 212));
}

#[test]
fn mocha_surface0_feeds_code_block_background() {
    let theme = TuiTheme::catppuccin_mocha();

    assert_eq!(theme.surface0, Color::Rgb(43, 44, 61));
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
fn row_user_bg_uses_darkened_surface0_for_all_flavors() {
    // (theme, expected darkened surface0, official Catppuccin surface0)
    let cases = [
        (
            TuiTheme::catppuccin_mocha(),
            Color::Rgb(43, 44, 61),
            Color::Rgb(49, 50, 68),
        ),
        (
            TuiTheme::catppuccin_latte(),
            Color::Rgb(196, 200, 212),
            Color::Rgb(204, 208, 218),
        ),
        (
            TuiTheme::catppuccin_frappe(),
            Color::Rgb(59, 63, 82),
            Color::Rgb(65, 69, 89),
        ),
        (
            TuiTheme::catppuccin_macchiato(),
            Color::Rgb(48, 52, 72),
            Color::Rgb(54, 58, 79),
        ),
    ];

    for (theme, expected, official) in cases {
        assert_eq!(theme.surface0, expected);
        assert_eq!(theme.row_user_bg, expected);
        assert!(
            is_strictly_darker(expected, official),
            "darkened surface0 {expected:?} must be darker than the official {official:?}"
        );
    }
}

fn is_strictly_darker(darker: Color, lighter: Color) -> bool {
    let (Color::Rgb(dr, dg, db), Color::Rgb(lr, lg, lb)) = (darker, lighter) else {
        return false;
    };
    dr < lr && dg < lg && db < lb
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
