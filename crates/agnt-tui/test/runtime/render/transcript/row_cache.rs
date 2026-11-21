//! The rendered-row cache as the pane's paint uses it: a warm frame costs the
//! cache-hit blit, a content change re-renders exactly its own entry, a
//! theme/width change re-renders everything, and every path paints the same
//! bytes as a cold cache does.

use super::*;

use crate::rendering::theme::{ThemeName, TuiTheme};
use crate::runtime::render::row_cache::RowCache;
use crate::runtime::render::transcript::{PaintWindow, paint_transcript_rows};
use crate::state::TranscriptStore;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

// -- Test Support

/// The width the panes paint at in this suite.
const WIDTH: usize = 52;

/// The viewport height: shorter than the fixture transcript, so the window sits
/// inside it and the paint skips rows.
const HEIGHT: usize = 14;

/// A reply long enough to fill the viewport and then some.
const LONG_REPLY: &str = "the answer opens here and runs long enough to fill the visible window\
and to push the rest of the transcript out of it, so the pane paints a window\n\
rather than every entry the store holds";

/// A transcript taller than the viewport: a user turn, a settled tool call, and
/// a long reply.
fn fixture_store(width: usize) -> TranscriptStore {
    let mut store = TranscriptStore::default();
    store.push_entry(user_entry("run the thing"));
    store.push_entry(settled_tool(tool_entry("read", "{\"path\":\"src/a.rs\"}")));
    store.push_entry(assistant_entry(LONG_REPLY));
    store.rebuild_height_index(width);
    store
}

/// The same entry with a terminal status, so its paint carries a fixed glyph
/// instead of the in-progress spinner.
fn settled_tool(mut entry: TranscriptEntry) -> TranscriptEntry {
    if let TranscriptEntry::Tool(tool) = &mut entry {
        tool.status = ItemStatus::Done;
    }
    entry
}

/// Paint the transcript's window `first_row..first_row + HEIGHT` at `width`
/// into a fresh canvas, at the frame clock's zero reading.
fn paint_window(
    store: &TranscriptStore,
    cache: &mut RowCache,
    theme: &TuiTheme,
    width: usize,
    first_row: usize,
) -> Buffer {
    paint_window_at(store, cache, theme, width, first_row, 0)
}

/// Paint the same window at `now_millis`. The frame clock is an input of the
/// paint, so a test advances it instead of waiting for the wall clock.
fn paint_window_at(
    store: &TranscriptStore,
    cache: &mut RowCache,
    theme: &TuiTheme,
    width: usize,
    first_row: usize,
    now_millis: u128,
) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width as u16, HEIGHT as u16));
    let window = PaintWindow {
        theme,
        width,
        first_row,
        height: HEIGHT,
        top: 0,
        now_millis,
    };
    paint_transcript_rows(store, &window, cache, &mut buffer);
    buffer
}

/// The row-0 text of a canvas, so a spinner glyph can be read back.
fn first_row_text(buffer: &Buffer) -> String {
    (0..buffer.area.width)
        .map(|x| {
            buffer
                .cell((x, 0))
                .map(|cell| cell.symbol().to_string())
                .unwrap_or_default()
        })
        .collect()
}

// -- Cache

/// WHEN a frame paints the same entries at the same width and theme as the
/// frame before it, THEN THE pane SHALL blit them from the cache and re-render
/// nothing — the bytes are the same either way.
#[test]
fn test_transcript_paint_row_cache_serves_an_unchanged_frame_from_the_cache() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let store = fixture_store(WIDTH);
    let mut cache = RowCache::default();

    // -- Exec: the cold frame parks every entry it paints.
    let cold = paint_window(&store, &mut cache, &theme, WIDTH, 0);
    let painted = cache.misses();
    assert!(painted > 1, "the fixture must paint several entries");
    assert_eq!(cache.hits(), 0, "a cold cache hits nothing");

    // -- Exec: the next frame paints the same window.
    let warm = paint_window(&store, &mut cache, &theme, WIDTH, 0);

    // -- Check: every painted entry came from the cache, and the frame is
    // byte-identical.
    assert_eq!(
        cache.misses(),
        painted,
        "the warm frame re-rendered nothing"
    );
    assert_eq!(cache.hits(), painted, "every painted entry was a hit");
    assert_eq!(warm, cold, "a cached frame paints the same bytes");
    Ok(())
}

/// WHEN one entry's content changes, THEN the next frame SHALL re-render that
/// entry alone: every other painted entry still hits the cache, and the bytes
/// match what a cold cache paints.
#[test]
fn test_transcript_paint_row_cache_rerenders_only_the_changed_entry() -> Result<()> {
    // -- Setup & Fixtures: a warm cache over a settled transcript.
    let theme = TuiTheme::default();
    let mut store = fixture_store(WIDTH);
    let mut cache = RowCache::default();
    paint_window(&store, &mut cache, &theme, WIDTH, 0);
    let tool_index = store.len() - 2;

    // -- Exec: the tool call records its completion display in place.
    let recorded = store.set_tool_result_at(
        tool_index,
        Display {
            title: Some("read".to_string()),
            sections: vec![DisplaySection {
                label: Some("read".to_string()),
                kind: ContentKind::Plain,
                content: "src/a.rs\nline two".to_string(),
                stats: None,
            }],
        },
    );
    assert!(recorded, "the fixture's tool entry must take a result");

    // -- Exec: a cold cache counts the entries this frame paints.
    let cold_painted = {
        let mut cold = RowCache::default();
        paint_window(&store, &mut cold, &theme, WIDTH, 0);
        cold.misses()
    };
    let misses_before = cache.misses();
    let hits_before = cache.hits();

    // -- Exec: the warm cache paints the same frame.
    let warm = paint_window(&store, &mut cache, &theme, WIDTH, 0);
    let mut cold = RowCache::default();
    let uncached = paint_window(&store, &mut cold, &theme, WIDTH, 0);

    // -- Check: exactly one entry re-rendered; the rest hit.
    assert_eq!(
        cache.misses() - misses_before,
        1,
        "only the changed entry re-renders"
    );
    assert_eq!(
        cache.hits() - hits_before,
        cold_painted.saturating_sub(1),
        "every other painted entry hits the cache"
    );
    assert_eq!(warm, uncached, "the cached frame paints the same bytes");
    Ok(())
}

/// WHEN a streamed chunk folds into an entry, THEN the next frame SHALL
/// re-render that entry alone — every other painted entry still hits the cache.
#[test]
fn test_transcript_paint_row_cache_rerenders_only_the_streamed_entry() -> Result<()> {
    // -- Setup & Fixtures: a warm cache, with the assistant reply streaming as
    // the transcript's last entry.
    let theme = TuiTheme::default();
    let mut store = fixture_store(WIDTH);
    let mut cache = RowCache::default();
    paint_window(&store, &mut cache, &theme, WIDTH, 0);
    let streamed_index = store.len() - 1;

    // -- Exec: a chunk folds into the entry, and the frame settles the index the
    // way the pane does before it paints.
    store.fold_stream_markdown_at(
        streamed_index,
        format!("{LONG_REPLY}\none more streamed line"),
    );
    store.ensure_height_index(WIDTH);
    let misses_before = cache.misses();
    let hits_before = cache.hits();
    let warm = paint_window(&store, &mut cache, &theme, WIDTH, 0);

    // -- Check: the folded entry re-rendered, the rest hit, and the frame
    // matches an uncached one.
    let mut cold = RowCache::default();
    let uncached = paint_window(&store, &mut cold, &theme, WIDTH, 0);
    assert_eq!(
        cache.misses() - misses_before,
        1,
        "only the streamed entry re-renders"
    );
    assert_eq!(
        cache.hits() - hits_before,
        cold.misses().saturating_sub(1),
        "every other painted entry hits the cache"
    );
    assert_eq!(warm, uncached, "the cached frame paints the same bytes");
    Ok(())
}

/// WHEN the theme changes, THEN the cache SHALL invalidate wholesale and the
/// next frame SHALL re-render every entry it paints.
#[test]
fn test_transcript_paint_row_cache_invalidates_every_entry_on_a_theme_change() -> Result<()> {
    // -- Setup & Fixtures: a warm cache over the default theme.
    let store = fixture_store(WIDTH);
    let mut cache = RowCache::default();
    let mut theme = TuiTheme::default();
    paint_window(&store, &mut cache, &theme, WIDTH, 0);
    let hits_before = cache.hits();

    // -- Exec: the user switches theme, and the next frame paints.
    theme = ThemeName::CatppuccinLatte.resolve();
    let switched = paint_window(&store, &mut cache, &theme, WIDTH, 0);

    // -- Check: the cache invalidated, nothing hit, and the frame is the
    // uncached frame.
    assert_eq!(cache.invalidations(), 1, "the theme change invalidates");
    assert_eq!(cache.hits(), hits_before, "no entry hit across the switch");
    let mut cold = RowCache::default();
    let uncached = paint_window(&store, &mut cold, &theme, WIDTH, 0);
    assert_eq!(switched, uncached, "the theme's rows paint the same bytes");
    Ok(())
}

/// WHEN the terminal width changes, THEN the cache SHALL invalidate wholesale
/// and the next frame SHALL re-render every entry at the new width.
#[test]
fn test_transcript_paint_row_cache_invalidates_every_entry_on_a_width_change() -> Result<()> {
    // -- Setup & Fixtures: a warm cache at one width.
    let theme = TuiTheme::default();
    let mut store = fixture_store(WIDTH);
    let mut cache = RowCache::default();
    paint_window(&store, &mut cache, &theme, WIDTH, 0);
    let hits_before = cache.hits();

    // -- Exec: a resize re-indexes the store and the next frame paints wider.
    let wider = WIDTH + 8;
    store.rebuild_height_index(wider);
    let resized = paint_window(&store, &mut cache, &theme, wider, 0);

    // -- Check: nothing hit, and the frame is the uncached frame.
    assert_eq!(cache.invalidations(), 1, "the width change invalidates");
    assert_eq!(cache.hits(), hits_before, "no entry hit across the resize");
    let mut cold = RowCache::default();
    let uncached = paint_window(&store, &mut cold, &theme, wider, 0);
    assert_eq!(resized, uncached, "the wider frame paints the same bytes");
    Ok(())
}

/// WHEN the cache is too small to hold the window, THEN it SHALL evict the
/// least-recently-used canvas and the paint SHALL stay byte-identical.
#[test]
fn test_transcript_paint_row_cache_eviction_keeps_the_output_identical() -> Result<()> {
    // -- Setup & Fixtures: a cache that can hold one entry's rows at a time.
    let theme = TuiTheme::default();
    let store = fixture_store(WIDTH);
    let mut cache = RowCache::with_limits(1, 4);

    // -- Exec
    let thrashing = paint_window(&store, &mut cache, &theme, WIDTH, 0);

    // -- Check: evictions happened, and the frame is the uncached frame.
    assert!(
        cache.evicted_total() > 0,
        "a one-entry cache must evict while it paints"
    );
    assert!(
        cache.parked_rows() <= 4,
        "the row budget holds after every insert"
    );
    let mut cold = RowCache::default();
    let uncached = paint_window(&store, &mut cold, &theme, WIDTH, 0);
    assert_eq!(thrashing, uncached, "eviction changes no painted byte");
    Ok(())
}

/// WHEN an entry is in progress, THEN its rows SHALL NOT be parked: the status
/// glyph animates, so a parked canvas would freeze the spinner.
#[test]
fn test_transcript_paint_row_cache_does_not_park_an_in_progress_entry() -> Result<()> {
    // -- Setup & Fixtures: one running tool call, the only entry.
    let theme = TuiTheme::default();
    let mut store = TranscriptStore::default();
    store.push_entry(tool_entry("read", "{\"path\":\"src/a.rs\"}"));
    store.rebuild_height_index(WIDTH);
    let mut cache = RowCache::default();

    // -- Exec: two frames one spinner step apart.
    let first = paint_window_at(&store, &mut cache, &theme, WIDTH, 0, 0);
    let second = paint_window_at(&store, &mut cache, &theme, WIDTH, 0, 120);

    // -- Check: the running entry is never cached, and its glyph advanced.
    assert_eq!(cache.hits(), 0, "a running entry is never a hit");
    assert_eq!(cache.misses(), 0, "a running entry is not a miss either");
    assert_eq!(cache.parked_len(), 0, "nothing is parked for it");
    assert_ne!(
        first_row_text(&first),
        first_row_text(&second),
        "the spinner must advance between two frames"
    );
    Ok(())
}
