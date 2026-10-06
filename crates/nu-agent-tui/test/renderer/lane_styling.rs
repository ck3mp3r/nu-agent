use super::support::{frame_ctx, leading_spaces, span_containing};
use crate::rendering::theme::{TuiTheme, hint_to_style};
use crate::tui_renderer::{lane_prefix_width, layout};
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::items::Banner;
use nu_agent_core::transcript::renderer::Renderable;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== Lane ownership of styling (task 46ca79fe) ==========

/// The lane decides styling, not the source: a Banner source on the assistant
/// `Lane::Blank` gets assistant styling. This pins the SRP fix —
/// `LaneContext::from` no longer inspects `block.source` to special-case
/// banners.
#[test]
fn layout_blank_lane_banner_uses_assistant_style_proving_lane_decides() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Banner {
            text: "NOTALOGO".to_string(),
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let span = span_containing(&lines, "NOTALOGO").ok_or("banner content span must exist")?;
    assert_eq!(
        span.style.fg, theme.role_assistant.fg,
        "Lane::Blank must apply assistant styling regardless of the source; got {:?}",
        span.style.fg
    );
    Ok(())
}

/// `Lane::SystemBlank` applies system styling to its content: the banner
/// lane. `Banner::lane()` returns this variant, so the renderer needs no
/// source inspection to style a banner.
#[test]
fn layout_system_blank_lane_uses_role_system() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Banner {
            text: "LOGOART".to_string(),
        },
        lane: Lane::SystemBlank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let span = span_containing(&lines, "LOGOART").ok_or("banner content span must exist")?;
    assert_eq!(
        span.style.fg, theme.role_system.fg,
        "Lane::SystemBlank must use role_system fg, got {:?}",
        span.style.fg
    );
    assert_eq!(
        span.style.bg, theme.row_system.bg,
        "Lane::SystemBlank row background must match row_system"
    );
    Ok(())
}

/// Every system-blank row carries the system row background, including all
/// banner lines.
#[test]
fn layout_system_blank_multi_line_banner_rows_use_row_system() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };
    let block = Block {
        source: banner.source(),
        lane: banner.lane(),
        fill: banner.fill(),
        status: None,
    };
    assert_eq!(
        banner.lane(),
        Lane::SystemBlank,
        "banner lane is SystemBlank"
    );

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    for (idx, span) in lines.iter().flat_map(|l| &l.spans).enumerate() {
        assert_eq!(
            span.style.bg, theme.row_system.bg,
            "banner row {idx} span '{}' must use row_system bg, got {:?}",
            span.content, span.style.bg
        );
    }
    Ok(())
}

/// Centering follows the lane, not the source: a non-banner source on
/// `Lane::SystemBlank` is centered exactly like a banner. Proves the centering
/// decision reads `block.lane` and never inspects `block.source`.
#[test]
fn layout_non_banner_source_on_system_blank_lane_is_centered() -> Result<()> {
    // -- Setup & Fixtures: a notice (never centered by source) on the banner
    // lane.
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "CENTERME".to_string(),
        },
        lane: Lane::SystemBlank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: centering prepends left padding, so row 0 carries more leading
    // whitespace than the bare lane prefix.
    let leading = leading_spaces(lines.first().ok_or("row 0 must exist")?);
    assert!(
        leading > lane_prefix_width(),
        "Lane::SystemBlank must center regardless of source; leading={leading}, prefix={}",
        lane_prefix_width()
    );
    Ok(())
}

/// Centering follows the lane, not the source: a banner source on the
/// assistant `Lane::Blank` is NOT centered. The lane already encodes "this is
/// a banner block" via `Lane::SystemBlank`.
#[test]
fn layout_banner_source_on_blank_lane_is_not_centered() -> Result<()> {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Banner {
            text: "NOTCENTERED".to_string(),
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: no centering padding is inserted, so the only leading
    // whitespace is the lane prefix itself.
    let leading = leading_spaces(lines.first().ok_or("row 0 must exist")?);
    assert_eq!(
        leading,
        lane_prefix_width(),
        "a Banner source on Lane::Blank must not be centered; leading={leading}"
    );
    Ok(())
}

/// A real banner block (source and lane from `Banner`) keeps its centering.
#[test]
fn layout_banner_on_system_blank_lane_is_centered() -> Result<()> {
    // -- Setup & Fixtures
    let banner = Banner {
        text: "LOGOLINE".to_string(),
    };
    let block = Block {
        source: banner.source(),
        lane: banner.lane(),
        fill: banner.fill(),
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let leading = leading_spaces(lines.first().ok_or("row 0 must exist")?);
    assert!(
        leading > lane_prefix_width(),
        "a banner block must stay centered; leading={leading}, prefix={}",
        lane_prefix_width()
    );
    Ok(())
}

/// Notice content is styled from the lane role style with `StyleHint::Normal`
/// (the pre-refactor `SystemMessage` styling), not `StyleHint::Meta`.
#[test]
fn layout_system_notice_content_uses_normal_hint_with_lane_role_style() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "system notice text".to_string(),
        },
        lane: Lane::Marker("\u{00b7}"),
        fill: Fill::None,
        status: None,
    };
    let expected = hint_to_style(&StyleHint::Normal, theme.role_system, &theme);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: the full style (fg + modifiers), not just the fg — `Meta`
    // shares a fg with some role styles but adds DIM.
    let span =
        span_containing(&lines, "system notice text").ok_or("notice content span must exist")?;
    assert_eq!(
        span.style, expected,
        "system notice content must use the Normal hint with role_system; got {:?}",
        span.style
    );
    assert_ne!(
        span.style,
        hint_to_style(&StyleHint::Meta, theme.role_system, &theme),
        "system notice content must not use the Meta hint"
    );
    Ok(())
}

/// Compaction notice content is styled from the compaction role style with
/// `StyleHint::Normal`.
#[test]
fn layout_compaction_notice_content_uses_normal_hint_with_lane_role_style() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compaction notice text".to_string(),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };
    let expected = hint_to_style(&StyleHint::Normal, theme.role_compaction, &theme);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: full-style equality. `role_compaction` and `tool_meta` share
    // the same fg but differ by the DIM modifier, so only the full style
    // catches a Meta regression here.
    let span = span_containing(&lines, "compaction notice text")
        .ok_or("notice content span must exist")?;
    assert_eq!(
        span.style, expected,
        "compaction notice content must use the Normal hint with role_compaction; got {:?}",
        span.style
    );
    assert_ne!(
        span.style,
        hint_to_style(&StyleHint::Meta, theme.role_compaction, &theme),
        "compaction notice content must not use the Meta hint"
    );
    Ok(())
}
