use ratatui::style::Modifier;

use crate::rendering::theme::{CatppuccinPalette, TuiTheme, hint_to_style};
use agnt_core::transcript::ir::StyleHint;

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

/// WHEN `hint_to_style` receives `MdHeading(1..=6)`, THE function SHALL return
/// the matching `theme.md_heading[level]`; WHEN it receives `MdHeading(0)` or
/// `MdHeading(> 6)`, THE function SHALL return `theme.md_heading[0]`.
#[test]
fn hint_to_style_maps_md_heading_levels() {
    let theme = TuiTheme::default();
    let role = theme.row_assistant;

    for level in 1..=6u8 {
        assert_eq!(
            hint_to_style(&StyleHint::MdHeading(level), role, &theme),
            theme.md_heading[usize::from(level)],
            "MdHeading({level}) must map to md_heading[{level}]"
        );
    }
    assert_eq!(
        hint_to_style(&StyleHint::MdHeading(0), role, &theme),
        theme.md_heading[0]
    );
    assert_eq!(
        hint_to_style(&StyleHint::MdHeading(7), role, &theme),
        theme.md_heading[0]
    );
}

/// WHEN `hint_to_style` receives `MdLink`, `MdLinkUrl`, `MdQuote`, or
/// `MdListMarker`, THE function SHALL return the matching theme field.
#[test]
fn hint_to_style_maps_md_link_url_quote_list_marker() {
    let theme = TuiTheme::default();
    let role = theme.row_assistant;

    assert_eq!(
        hint_to_style(&StyleHint::MdLink, role, &theme),
        theme.md_link
    );
    assert_eq!(
        hint_to_style(&StyleHint::MdLinkUrl, role, &theme),
        theme.md_link_url
    );
    assert_eq!(
        hint_to_style(&StyleHint::MdQuote, role, &theme),
        theme.md_quote
    );
    assert_eq!(
        hint_to_style(&StyleHint::MdListMarker, role, &theme),
        theme.md_list_marker
    );
}

// ========== recolored emphasis and syntax hints (task 689d0ec2) ==========

/// WHEN `hint_to_style` receives `MdBold`, `MdItalic`, or `MdBoldItalic`, THE
/// function SHALL return red foreground with BOLD, ITALIC, or both modifiers
/// respectively, for every flavor.
#[test]
fn hint_to_style_maps_emphasis_hints_to_red_for_all_flavors() {
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
        let role = theme.row_assistant;

        let bold = hint_to_style(&StyleHint::MdBold, role, &theme);
        assert_eq!(bold.fg, Some(palette.red), "MdBold must be red");
        assert!(bold.add_modifier.contains(Modifier::BOLD));
        assert!(!bold.add_modifier.contains(Modifier::ITALIC));

        let italic = hint_to_style(&StyleHint::MdItalic, role, &theme);
        assert_eq!(italic.fg, Some(palette.red), "MdItalic must be red");
        assert!(italic.add_modifier.contains(Modifier::ITALIC));
        assert!(!italic.add_modifier.contains(Modifier::BOLD));

        let both = hint_to_style(&StyleHint::MdBoldItalic, role, &theme);
        assert_eq!(both.fg, Some(palette.red), "MdBoldItalic must be red");
        assert!(both.add_modifier.contains(Modifier::BOLD));
        assert!(both.add_modifier.contains(Modifier::ITALIC));
    }
}

/// WHEN `hint_to_style` receives `MdCodeVariable`, `MdCodeConstant`,
/// `MdCodeOperator`, `MdCodePunctuation`, `MdCodeComment`, or `MdInlineCode`,
/// THE function SHALL return the recolored syntax styles.
#[test]
fn hint_to_style_maps_code_hints_to_recolored_syntax_styles() {
    let theme = TuiTheme::default();
    let role = theme.row_assistant;
    let palette = CatppuccinPalette::mocha();

    assert_eq!(
        hint_to_style(&StyleHint::MdCodeVariable, role, &theme).fg,
        Some(palette.text)
    );
    assert_eq!(
        hint_to_style(&StyleHint::MdCodeConstant, role, &theme).fg,
        Some(palette.peach)
    );
    assert_eq!(
        hint_to_style(&StyleHint::MdCodeOperator, role, &theme).fg,
        Some(palette.sky)
    );
    assert_eq!(
        hint_to_style(&StyleHint::MdCodePunctuation, role, &theme).fg,
        Some(palette.overlay2)
    );

    let comment = hint_to_style(&StyleHint::MdCodeComment, role, &theme);
    assert_eq!(comment.fg, Some(palette.overlay2));
    assert!(comment.add_modifier.contains(Modifier::ITALIC));
    assert!(!comment.add_modifier.contains(Modifier::DIM));

    let inline = hint_to_style(&StyleHint::MdInlineCode, role, &theme);
    assert_eq!(inline.fg, Some(palette.green));
    assert!(!inline.add_modifier.contains(Modifier::DIM));
}
