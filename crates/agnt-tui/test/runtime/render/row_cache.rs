//! Unit tests for the rendered-row cache: what a key matches, when a frame
//! drops the parked rows wholesale, and which canvas the bound evicts first.

use super::*;

use agnt_render::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::state::EntryVersion;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- Test Support

/// The cache under test, with bounds wide enough that only the test's own
/// inserts matter.
fn roomy_cache() -> RowCache {
    RowCache::with_limits(16, 1024)
}

/// A `width` x `height` canvas whose every cell carries `symbol`, so two
/// canvases are equal only when their geometry and their glyphs agree.
fn canvas(width: u16, height: u16, symbol: &str) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    for y in 0..height {
        for x in 0..width {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(symbol);
            }
        }
    }
    buffer
}

/// The cache identity of an entry that has never changed: id and revision take
/// the same value, as a fresh store entry's do.
fn version(id: u64) -> EntryVersion {
    EntryVersion { id, revision: id }
}

fn theme() -> Theme {
    Theme::default()
}

// -- Cache

/// WHEN a frame asks for an entry it parked at the same revision and geometry,
/// THEN the cache SHALL hand back the parked canvas — that frame re-renders
/// nothing.
#[test]
fn test_render_row_cache_get_returns_the_parked_canvas() -> Result<()> {
    // -- Setup & Fixtures: one entry's canvas parked at a known geometry.
    let mut cache = roomy_cache();
    cache.begin_frame(40, theme());
    let parked = canvas(40, 3, "a");
    cache.insert(version(7), 3, 0, parked.clone());

    // -- Exec
    let hit = cache.get(version(7), 3, 0);

    // -- Check: the parked bytes come back untouched, and the counters say a
    // hit was served with no miss beside it.
    assert_eq!(hit, Some(&parked), "the parked canvas must come back as is");
    assert_eq!(cache.hits(), 1, "one lookup served from the cache");
    assert_eq!(cache.misses(), 0, "a served lookup is not a miss");
    Ok(())
}

/// WHEN an entry's revision moves, THEN its parked canvas SHALL stop matching:
/// the rows belong to content that is no longer on screen.
#[test]
fn test_render_row_cache_get_misses_when_the_revision_moves() -> Result<()> {
    // -- Setup & Fixtures
    let mut cache = roomy_cache();
    cache.begin_frame(40, theme());
    cache.insert(version(7), 3, 0, canvas(40, 3, "a"));

    // -- Exec: the same entry, one content change later.
    let hit = cache.get(EntryVersion { id: 7, revision: 8 }, 3, 0);

    // -- Check
    assert_eq!(hit, None, "a moved revision must not serve the old rows");
    assert_eq!(cache.misses(), 1);
    assert_eq!(cache.hits(), 0);
    Ok(())
}

/// WHEN the geometry a frame asks for differs from the parked geometry, THEN
/// the lookup SHALL miss: the canvas holds rows for another rect.
#[test]
fn test_render_row_cache_get_misses_when_the_geometry_changes() -> Result<()> {
    // -- Setup & Fixtures
    let mut cache = roomy_cache();
    cache.begin_frame(40, theme());
    cache.insert(version(7), 3, 1, canvas(40, 3, "a"));

    // -- Exec & Check: a different row count, then a different content offset.
    assert_eq!(
        cache.get(version(7), 4, 1),
        None,
        "a taller rect must not serve the shorter canvas"
    );
    assert_eq!(
        cache.get(version(7), 3, 0),
        None,
        "another content offset must not serve the parked canvas"
    );
    assert_eq!(cache.misses(), 2);
    Ok(())
}

/// WHEN two frames paint at the same width and theme, THEN the parked rows SHALL
/// survive: nothing about them is obsolete.
#[test]
fn test_render_row_cache_begin_frame_keeps_its_rows_for_the_same_inputs() -> Result<()> {
    // -- Setup & Fixtures
    let mut cache = roomy_cache();
    cache.begin_frame(40, theme());
    cache.insert(version(7), 3, 0, canvas(40, 3, "a"));

    // -- Exec: a second frame with the same width and theme.
    cache.begin_frame(40, theme());

    // -- Check
    assert_eq!(
        cache.invalidations(),
        0,
        "an unchanged frame invalidates nothing"
    );
    assert!(
        cache.get(version(7), 3, 0).is_some(),
        "the parked canvas must still be served"
    );
    Ok(())
}

/// WHEN the theme changes, THEN every parked canvas SHALL be dropped: the rows
/// were styled for the previous theme.
#[test]
fn test_render_row_cache_begin_frame_invalidates_on_a_theme_change() -> Result<()> {
    // -- Setup & Fixtures
    let mut cache = roomy_cache();
    cache.begin_frame(40, Theme::default());
    cache.insert(version(7), 3, 0, canvas(40, 3, "a"));

    // -- Exec
    let other = Theme {
        text: agnt_render::Color::rgb(1, 2, 3),
        ..Theme::default()
    };
    cache.begin_frame(40, other);

    // -- Check
    assert_eq!(cache.invalidations(), 1, "a theme change invalidates");
    assert_eq!(cache.parked_len(), 0, "every canvas is dropped");
    assert_eq!(cache.parked_rows(), 0, "the row budget is freed");
    assert_eq!(
        cache.get(version(7), 3, 0),
        None,
        "no canvas survives a theme change"
    );
    Ok(())
}

/// WHEN the width changes, THEN every parked canvas SHALL be dropped: the rows
/// were wrapped at the previous width.
#[test]
fn test_render_row_cache_begin_frame_invalidates_on_a_width_change() -> Result<()> {
    // -- Setup & Fixtures
    let mut cache = roomy_cache();
    cache.begin_frame(40, theme());
    cache.insert(version(7), 3, 0, canvas(40, 3, "a"));

    // -- Exec
    cache.begin_frame(48, theme());

    // -- Check
    assert_eq!(cache.invalidations(), 1, "a width change invalidates");
    assert_eq!(
        cache.get(version(7), 3, 0),
        None,
        "no canvas survives a width change"
    );
    Ok(())
}

/// WHEN the entry bound is exceeded, THEN the canvas no frame has read for
/// longest SHALL go first.
#[test]
fn test_render_row_cache_evicts_the_least_recently_used_entry() -> Result<()> {
    // -- Setup & Fixtures: two entries parked, the first one read afterwards.
    let mut cache = RowCache::with_limits(2, 1024);
    cache.begin_frame(40, theme());
    cache.insert(version(1), 1, 0, canvas(4, 1, "a"));
    cache.insert(version(2), 1, 0, canvas(4, 1, "b"));
    assert!(
        cache.get(version(1), 1, 0).is_some(),
        "the first entry is read"
    );

    // -- Exec: a third entry overflows the entry bound.
    cache.insert(version(3), 1, 0, canvas(4, 1, "c"));

    // -- Check: the untouched entry went, the read one stayed.
    assert_eq!(cache.evicted_total(), 1, "one canvas is evicted");
    assert_eq!(cache.parked_len(), 2, "the bound holds");
    assert!(
        cache.get(version(1), 1, 0).is_some(),
        "the recent entry stays"
    );
    assert!(
        cache.get(version(2), 1, 0).is_none(),
        "the least recently used entry went first"
    );
    Ok(())
}

/// WHEN the row budget would be exceeded, THEN the cache SHALL evict until the
/// parked rows fit — a row bound is the memory bound that matters.
#[test]
fn test_render_row_cache_evicts_when_the_row_budget_is_exceeded() -> Result<()> {
    // -- Setup & Fixtures: a ten-row budget.
    let mut cache = RowCache::with_limits(64, 10);
    cache.begin_frame(40, theme());
    cache.insert(version(1), 6, 0, canvas(4, 6, "a"));
    assert_eq!(cache.parked_rows(), 6, "the first canvas is parked");

    // -- Exec: a second canvas takes the total past the budget.
    cache.insert(version(2), 6, 0, canvas(4, 6, "b"));

    // -- Check: the older canvas went, so the parked rows fit again.
    assert_eq!(cache.parked_rows(), 6, "the row budget holds");
    assert_eq!(cache.evicted_total(), 1);
    assert!(
        cache.get(version(2), 6, 0).is_some(),
        "the newest canvas stays"
    );
    Ok(())
}
