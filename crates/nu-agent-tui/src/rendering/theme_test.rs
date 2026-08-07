use ratatui::style::{Color, Modifier};

use crate::rendering::theme::{ThemeName, TuiTheme};

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
const MOCHA_USER_BG: Color = Color::Rgb(38, 38, 55);

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
    assert_eq!(theme.row_user_bg, MOCHA_USER_BG);
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
}

#[test]
fn catppuccin_latte_maps_to_latte_palette() {
    let theme = TuiTheme::catppuccin_latte();

    assert_eq!(theme.syntax_keyword.fg, Some(Color::Rgb(136, 57, 239)));
    assert_eq!(theme.role_user.fg, Some(Color::Rgb(114, 135, 253)));
    assert_eq!(theme.role_assistant.fg, Some(Color::Rgb(114, 135, 253)));
    assert_eq!(theme.status_failed.fg, Some(Color::Rgb(210, 15, 57)));
    assert_eq!(theme.status_done.fg, Some(Color::Rgb(64, 160, 43)));
    assert_eq!(theme.row_user_bg, Color::Rgb(228, 230, 236));
}
