use ratatui::style::Color;

use crate::rendering::theme::{CatppuccinPalette, TuiTheme, diff_tint_to_bg};
use agnt_core::transcript::ir::DiffTint;

// ========== diff tint backgrounds (task 669b4dac) ==========

/// WHEN `diff_tint_to_bg` receives `DiffTint::Add`, THE function SHALL return
/// `Some(theme.diff_add_bg)`.
#[test]
fn diff_tint_to_bg_add_returns_add_background() {
    let theme = TuiTheme::default();

    assert_eq!(
        diff_tint_to_bg(&DiffTint::Add, &theme),
        Some(theme.diff_add_bg)
    );
}

/// WHEN `diff_tint_to_bg` receives `DiffTint::Remove`, THE function SHALL
/// return `Some(theme.diff_remove_bg)`.
#[test]
fn diff_tint_to_bg_remove_returns_remove_background() {
    let theme = TuiTheme::default();

    assert_eq!(
        diff_tint_to_bg(&DiffTint::Remove, &theme),
        Some(theme.diff_remove_bg)
    );
}

/// WHEN `diff_tint_to_bg` receives `DiffTint::Context` and the theme leaves
/// `diff_context_bg` as `None`, THE function SHALL return `None`.
#[test]
fn diff_tint_to_bg_context_returns_none_by_default() {
    let theme = TuiTheme::default();

    assert_eq!(theme.diff_context_bg, None);
    assert_eq!(diff_tint_to_bg(&DiffTint::Context, &theme), None);
}

/// WHEN `diff_tint_to_bg` receives `DiffTint::Context` and the theme sets
/// `diff_context_bg`, THE function SHALL return that colour.
#[test]
fn diff_tint_to_bg_context_returns_configured_background() {
    let theme = TuiTheme {
        diff_context_bg: Some(Color::Rgb(1, 2, 3)),
        ..TuiTheme::default()
    };

    assert_eq!(
        diff_tint_to_bg(&DiffTint::Context, &theme),
        Some(Color::Rgb(1, 2, 3))
    );
}

/// WHEN the theme is built from a palette, THE add/remove diff backgrounds
/// SHALL be dimmed tints of the palette green/red — pulled toward the theme
/// base so syntax foreground colours stay readable on top.
#[test]
fn diff_backgrounds_are_dimmed_palette_accents() {
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
        assert!(
            is_tint_of(theme.diff_add_bg, palette.green, palette.base),
            "add tint {:?} must be a dimmed tint of the palette green {:?} over base {:?}",
            theme.diff_add_bg,
            palette.green,
            palette.base
        );
        assert!(
            is_tint_of(theme.diff_remove_bg, palette.red, palette.base),
            "remove tint {:?} must be a dimmed tint of the palette red {:?} over base {:?}",
            theme.diff_remove_bg,
            palette.red,
            palette.base
        );
    }
}

/// Whether `tint` is a dimmed version of `accent` over `base`: an RGB colour
/// that is strictly closer to the background than the accent is, i.e. the
/// accent pulled toward the base so the syntax foreground stays readable.
fn is_tint_of(tint: Color, accent: Color, base: Color) -> bool {
    let (Color::Rgb(tr, tg, tb), Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) =
        (tint, accent, base)
    else {
        return false;
    };
    let dist_sq = |r: u8, g: u8, b: u8| {
        let d = |a: u8, b: u8| i64::from(a) - i64::from(b);
        d(r, br).pow(2) + d(g, bg).pow(2) + d(b, bb).pow(2)
    };
    tint != accent && dist_sq(tr, tg, tb) < dist_sq(ar, ag, ab)
}
