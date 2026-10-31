//! Theme conversion: the TUI theme's styles projected onto the backend-neutral
//! surfaces every widget paints with.
//!
//! The conversion is the one place a terminal style becomes a semantic surface,
//! so it is pinned here rather than in the widget modules.

use super::support::*;
use agnt_render::Theme;
use ratatui::style::{Color as TermColor, Style};

use crate::rendering::theme::TuiTheme;

/// The terminal color a TUI style's foreground paints, or black when the style
/// carries none.
fn fg_of(style: Style) -> agnt_render::Color {
    color_of(style.fg.unwrap_or(TermColor::Reset))
}

/// A terminal color as the backend-neutral color, or black when it names no
/// palette color.
fn color_of(color: TermColor) -> agnt_render::Color {
    match color {
        TermColor::Rgb(r, g, b) => agnt_render::Color::rgb(r, g, b),
        _ => agnt_render::Color::default(),
    }
}

/// WHEN the TUI theme converts, THE surfaces SHALL resolve onto the TUI styles
/// that carry them: base, body text, muted, idle, accent, the user lane accent,
/// the running/success/error marks, the user and code surfaces, notice, banner,
/// the two diff *backgrounds*, and the markdown palette.
#[test]
fn test_renderer_lane_theme_conversion_maps_every_surface() -> Result<()> {
    // -- Setup & Fixtures
    let tui = TuiTheme::default();

    // -- Exec
    let theme = Theme::from(&tui);

    // -- Check
    assert_eq!(
        theme.base,
        color_of(tui.base),
        "the pane base comes from the TUI base color"
    );
    assert_eq!(theme.text, fg_of(tui.row_assistant), "body text");
    assert_eq!(theme.muted, fg_of(tui.tool_meta), "muted text");
    assert_eq!(theme.idle, fg_of(tui.status_queued), "the idle mark");
    assert_eq!(theme.accent, fg_of(tui.role_tool), "the lane accent");
    assert_eq!(
        theme.user_accent,
        fg_of(tui.lane_prefix_user),
        "the user lane accent"
    );
    assert_eq!(theme.running, fg_of(tui.status_running), "the running mark");
    assert_eq!(theme.success, fg_of(tui.status_done), "the done mark");
    assert_eq!(theme.error, fg_of(tui.status_failed), "the failed mark");
    assert_eq!(
        theme.user_surface,
        color_of(tui.row_user_bg),
        "the user surface"
    );
    assert_eq!(
        theme.code_surface,
        color_of(tui.surface0),
        "the code surface"
    );
    assert_eq!(theme.notice, fg_of(tui.role_system), "notice text");
    assert_eq!(theme.banner, fg_of(tui.role_system), "banner text");
    assert_eq!(
        theme.diff_add_bg,
        color_of(tui.diff_add_bg),
        "the add surface"
    );
    assert_eq!(
        theme.diff_remove_bg,
        color_of(tui.diff_remove_bg),
        "the remove surface"
    );
    assert_eq!(
        theme.markdown.code_keyword,
        fg_of(tui.syntax_keyword),
        "the syntax keyword"
    );
    assert_eq!(
        theme.markdown.inline_code,
        fg_of(tui.inline_code),
        "inline code"
    );
    for level in 0..7usize {
        assert_eq!(
            theme.markdown.heading[level],
            fg_of(tui.md_heading[level]),
            "md_heading[{level}]"
        );
    }
    Ok(())
}

/// WHEN two TUI palettes convert, THE neutral surfaces SHALL differ wherever the
/// palettes do — a conversion that dropped a surface would collapse them.
#[test]
fn test_renderer_lane_theme_conversion_keeps_palettes_distinct() -> Result<()> {
    // -- Setup & Fixtures
    let mocha = TuiTheme::catppuccin_mocha();
    let latte = TuiTheme::catppuccin_latte();

    // -- Exec
    let mocha = Theme::from(&mocha);
    let latte = Theme::from(&latte);

    // -- Check
    assert_ne!(mocha, latte, "the two palettes must stay distinct");
    for (name, left, right) in [
        ("base", mocha.base, latte.base),
        ("text", mocha.text, latte.text),
        ("muted", mocha.muted, latte.muted),
        ("idle", mocha.idle, latte.idle),
        ("accent", mocha.accent, latte.accent),
        ("user_accent", mocha.user_accent, latte.user_accent),
        ("running", mocha.running, latte.running),
        ("success", mocha.success, latte.success),
        ("error", mocha.error, latte.error),
        ("user_surface", mocha.user_surface, latte.user_surface),
        ("code_surface", mocha.code_surface, latte.code_surface),
        ("diff_add_bg", mocha.diff_add_bg, latte.diff_add_bg),
        ("diff_remove_bg", mocha.diff_remove_bg, latte.diff_remove_bg),
        (
            "markdown.emphasis",
            mocha.markdown.emphasis,
            latte.markdown.emphasis,
        ),
        ("markdown.quote", mocha.markdown.quote, latte.markdown.quote),
    ] {
        assert_ne!(left, right, "the {name} surface must follow the palette");
    }
    Ok(())
}
