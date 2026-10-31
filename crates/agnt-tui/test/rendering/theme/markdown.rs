use ratatui::style::Modifier;

use crate::rendering::theme::{CatppuccinPalette, TuiTheme};
use crate::widgets::text::hint_style;
use agnt_core::transcript::ir::StyleHint;
use agnt_render::Theme;

/// The style one hint paints with under the TUI theme: the widgets' single
/// vocabulary, resolved through the backend-neutral theme.
fn style_of(hint: &StyleHint, theme: &TuiTheme) -> ratatui::style::Style {
    hint_style(hint, theme.row_assistant, &Theme::from(theme))
}

// ========== markdown element styles (task 6b8be032) ==========

/// WHEN the theme is built from a palette, THE `md_heading` array SHALL carry
/// the generic bold blue style at index 0 and the per-level rainbow at
/// indices 1..=6 (red, peach, yellow, green, sapphire, lavender), each bold,
/// for every flavor.
#[test]
fn md_heading_styles_map_to_catppuccin_colors_for_all_flavors() {
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
        let expected = [
            palette.blue,
            palette.red,
            palette.peach,
            palette.yellow,
            palette.green,
            palette.sapphire,
            palette.lavender,
        ];
        for (level, color) in expected.into_iter().enumerate() {
            assert_eq!(
                theme.md_heading[level].fg,
                Some(color),
                "md_heading[{level}] fg must be {color:?}"
            );
            assert!(
                theme.md_heading[level]
                    .add_modifier
                    .contains(Modifier::BOLD),
                "md_heading[{level}] must be bold"
            );
        }
    }
}

/// WHEN the theme is built from a palette, THE `md_link`, `md_link_url`,
/// `md_quote`, and `md_list_marker` styles SHALL carry the lavender, italic
/// underlined blue, pink, and teal styles respectively, for every flavor.
#[test]
fn md_link_url_quote_list_marker_map_to_catppuccin_colors_for_all_flavors() {
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
        assert_eq!(theme.md_link.fg, Some(palette.lavender));
        assert_eq!(theme.md_link_url.fg, Some(palette.blue));
        assert!(
            theme.md_link_url.add_modifier.contains(Modifier::ITALIC),
            "md_link_url must be italic"
        );
        assert!(
            theme
                .md_link_url
                .add_modifier
                .contains(Modifier::UNDERLINED),
            "md_link_url must be underlined"
        );
        assert_eq!(theme.md_quote.fg, Some(palette.pink));
        assert_eq!(theme.md_list_marker.fg, Some(palette.teal));
    }
}

/// WHEN `hint_style` receives `MdHeading(1..=6)`, THE function SHALL return
/// the matching `theme.md_heading[level]`; WHEN it receives `MdHeading(0)` or
/// `MdHeading(> 6)`, THE function SHALL return `theme.md_heading[0]`.
#[test]
fn hint_style_maps_md_heading_levels() {
    let theme = TuiTheme::default();

    for level in 1..=6u8 {
        let style = style_of(&StyleHint::MdHeading(level), &theme);
        assert_eq!(style.fg, theme.md_heading[usize::from(level)].fg);
        assert!(
            style.add_modifier.contains(Modifier::BOLD),
            "MdHeading({level}) must stay bold"
        );
    }
    assert_eq!(
        style_of(&StyleHint::MdHeading(0), &theme).fg,
        theme.md_heading[0].fg
    );
    assert_eq!(
        style_of(&StyleHint::MdHeading(7), &theme).fg,
        theme.md_heading[0].fg
    );
}

/// WHEN the hint vocabulary receives `MdLink`, `MdLinkUrl`, `MdQuote`, or
/// `MdListMarker`, THE style SHALL carry the matching palette color.
#[test]
fn hint_style_maps_md_link_url_quote_list_marker() {
    let theme = TuiTheme::default();

    assert_eq!(style_of(&StyleHint::MdLink, &theme).fg, theme.md_link.fg);
    let url = style_of(&StyleHint::MdLinkUrl, &theme);
    assert_eq!(url.fg, theme.md_link_url.fg);
    assert!(url.add_modifier.contains(Modifier::ITALIC));
    assert!(url.add_modifier.contains(Modifier::UNDERLINED));
    assert_eq!(style_of(&StyleHint::MdQuote, &theme).fg, theme.md_quote.fg);
    assert_eq!(
        style_of(&StyleHint::MdListMarker, &theme).fg,
        theme.md_list_marker.fg
    );
}

// ========== recolored emphasis and syntax hints (task 689d0ec2) ==========

/// WHEN `hint_style` receives `MdBold`, `MdItalic`, or `MdBoldItalic`, THE
/// function SHALL return red foreground with BOLD, ITALIC, or both modifiers
/// respectively, for every flavor.
#[test]
fn hint_style_maps_emphasis_hints_to_red_for_all_flavors() {
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
        let bold = style_of(&StyleHint::MdBold, &theme);
        assert_eq!(bold.fg, Some(palette.red), "MdBold must be red");
        assert!(bold.add_modifier.contains(Modifier::BOLD));
        assert!(!bold.add_modifier.contains(Modifier::ITALIC));

        let italic = style_of(&StyleHint::MdItalic, &theme);
        assert_eq!(italic.fg, Some(palette.red), "MdItalic must be red");
        assert!(italic.add_modifier.contains(Modifier::ITALIC));
        assert!(!italic.add_modifier.contains(Modifier::BOLD));

        let both = style_of(&StyleHint::MdBoldItalic, &theme);
        assert_eq!(both.fg, Some(palette.red), "MdBoldItalic must be red");
        assert!(both.add_modifier.contains(Modifier::BOLD));
        assert!(both.add_modifier.contains(Modifier::ITALIC));
    }
}

/// WHEN `hint_style` receives `MdCodeVariable`, `MdCodeConstant`,
/// `MdCodeOperator`, `MdCodePunctuation`, `MdCodeComment`, or `MdInlineCode`,
/// THE function SHALL return the recolored syntax styles.
#[test]
fn hint_style_maps_code_hints_to_recolored_syntax_styles() {
    let theme = TuiTheme::default();
    let palette = CatppuccinPalette::mocha();

    assert_eq!(
        style_of(&StyleHint::MdCodeVariable, &theme).fg,
        Some(palette.text)
    );
    assert_eq!(
        style_of(&StyleHint::MdCodeConstant, &theme).fg,
        Some(palette.peach)
    );
    assert_eq!(
        style_of(&StyleHint::MdCodeOperator, &theme).fg,
        Some(palette.sky)
    );
    assert_eq!(
        style_of(&StyleHint::MdCodePunctuation, &theme).fg,
        Some(palette.overlay2)
    );

    let comment = style_of(&StyleHint::MdCodeComment, &theme);
    assert_eq!(comment.fg, Some(palette.overlay2));
    assert!(comment.add_modifier.contains(Modifier::ITALIC));
    assert!(!comment.add_modifier.contains(Modifier::DIM));

    let inline = style_of(&StyleHint::MdInlineCode, &theme);
    assert_eq!(inline.fg, Some(palette.green));
    assert!(!inline.add_modifier.contains(Modifier::DIM));
}
