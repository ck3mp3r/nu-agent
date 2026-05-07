use ratatui::style::{Color, Modifier};

use crate::commands::agent::ui::tui::rendering::theme::TuiTheme;

const CTP_MOCHA_RED: Color = Color::Rgb(243, 139, 168);
const CTP_MOCHA_YELLOW: Color = Color::Rgb(249, 226, 175);
const CTP_MOCHA_GREEN: Color = Color::Rgb(166, 227, 161);
const CTP_MOCHA_BLUE: Color = Color::Rgb(137, 180, 250);
const CTP_MOCHA_SAPPHIRE: Color = Color::Rgb(116, 199, 236);
const CTP_MOCHA_LAVENDER: Color = Color::Rgb(180, 190, 254);
const CTP_MOCHA_MAUVE: Color = Color::Rgb(203, 166, 247);
const CTP_MOCHA_PEACH: Color = Color::Rgb(250, 179, 135);
const CTP_MOCHA_OVERLAY0: Color = Color::Rgb(108, 112, 134);
const CTP_MOCHA_OVERLAY1: Color = Color::Rgb(127, 132, 156);

#[test]
fn syntax_channels_map_to_catppuccin_mocha_colors_and_styles() {
    let theme = TuiTheme::default();

    assert_eq!(theme.syntax_keyword.fg, Some(CTP_MOCHA_MAUVE));
    assert_eq!(theme.syntax_type.fg, Some(CTP_MOCHA_YELLOW));
    assert_eq!(theme.syntax_function.fg, Some(CTP_MOCHA_BLUE));
    assert_eq!(theme.syntax_variable.fg, Some(CTP_MOCHA_LAVENDER));
    assert_eq!(theme.syntax_constant.fg, Some(CTP_MOCHA_RED));
    assert_eq!(theme.syntax_string.fg, Some(CTP_MOCHA_GREEN));
    assert_eq!(theme.syntax_number.fg, Some(CTP_MOCHA_PEACH));
    assert_eq!(theme.syntax_operator.fg, Some(CTP_MOCHA_SAPPHIRE));
    assert_eq!(theme.syntax_punctuation.fg, Some(CTP_MOCHA_OVERLAY1));
    assert_eq!(theme.syntax_comment.fg, Some(CTP_MOCHA_OVERLAY0));
    assert!(theme.syntax_comment.add_modifier.contains(Modifier::DIM));
}

#[test]
fn existing_role_and_status_channels_remain_unchanged() {
    let theme = TuiTheme::default();

    assert_eq!(theme.role_user.fg, Some(CTP_MOCHA_BLUE));
    assert_eq!(theme.role_assistant.fg, Some(CTP_MOCHA_LAVENDER));
    assert_eq!(theme.role_system.fg, Some(CTP_MOCHA_YELLOW));
    assert_eq!(theme.role_tool.fg, Some(CTP_MOCHA_MAUVE));

    assert_eq!(theme.status_queued.fg, Some(CTP_MOCHA_OVERLAY0));
    assert_eq!(theme.status_running.fg, Some(CTP_MOCHA_SAPPHIRE));
    assert_eq!(theme.status_done.fg, Some(CTP_MOCHA_GREEN));
    assert_eq!(theme.status_failed.fg, Some(CTP_MOCHA_RED));
    assert_eq!(theme.status_cancelled.fg, Some(CTP_MOCHA_OVERLAY0));
}
