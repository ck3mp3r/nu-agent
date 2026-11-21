//! The rendered-row cache: one entry's painted canvas, parked until its
//! content, the pane width, or the theme changes.
//!
//! Painting an entry means projecting its markdown, wrapping every line, and
//! styling every cell. A busy-phase frame runs every 150 ms and repaints the
//! same window while a call is in flight, so that work repeats identically
//! frame after frame. The cache parks it: a frame whose entry content, width,
//! and theme all held still copies the parked canvas instead of re-rendering.
//!
//! The key is the store's [`EntryVersion`]: an id that is stable for the
//! entry's lifetime and a revision that moves on every content change. Entry
//! indices key nothing — cap eviction shifts them, so a canvas addressed by
//! index would be served for a different entry. The parked value carries the
//! geometry it was painted at, and a frame whose width or theme differs from the
//! last frame drops every canvas wholesale.
//!
//! Memory is bounded both ways: parked entries and parked rows. Overflow evicts
//! least-recently-used, and an eviction costs only a re-render — the painted
//! bytes are the same either way.

// region:    --- Modules

use std::collections::HashMap;

use agnt_render::Theme;
use ratatui::buffer::Buffer;

use crate::state::EntryVersion;

// endregion: --- Modules

// region:    --- Types

/// The frame-wide paint inputs every parked canvas depends on. A change in
/// either makes all of them obsolete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameIdentity {
    width: usize,
    theme: Theme,
}

/// One entry's painted canvas, and the geometry it was painted at.
#[derive(Debug, Clone)]
struct ParkedCanvas {
    /// The content revision the canvas was painted from.
    revision: u64,
    /// The row count of the rect the canvas covers.
    rows: usize,
    /// The rect's content offset.
    content_offset: usize,
    canvas: Buffer,
    /// The access tick of the last frame that read this canvas.
    last_used: u64,
}

/// The cache itself: what each entry last painted, and what the frames have
/// asked it for.
#[derive(Debug)]
pub(crate) struct RowCache {
    /// The paint inputs of the previous frame, or `None` before the first one.
    frame: Option<FrameIdentity>,
    /// One canvas per entry id.
    parked: HashMap<u64, ParkedCanvas>,
    parked_rows: usize,
    /// The access clock behind the LRU order.
    tick: u64,
    hits: usize,
    misses: usize,
    evicted: usize,
    invalidations: usize,
    max_entries: usize,
    max_rows: usize,
}

// endregion: --- Types

// region:    --- Constructors

impl Default for RowCache {
    fn default() -> Self {
        Self::with_limits(MAX_PARKED_ENTRIES, MAX_PARKED_ROWS)
    }
}

impl RowCache {
    /// A cache bounded at `max_entries` canvases and `max_rows` rows. The row
    /// bound is the one that tracks memory: a canvas costs its width times its
    /// row count in cells.
    pub(crate) fn with_limits(max_entries: usize, max_rows: usize) -> Self {
        Self {
            frame: None,
            parked: HashMap::new(),
            parked_rows: 0,
            tick: 0,
            hits: 0,
            misses: 0,
            evicted: 0,
            invalidations: 0,
            max_entries,
            max_rows,
        }
    }
}

// endregion: --- Constructors

// region:    --- Methods

impl RowCache {
    /// Bring the cache to one frame's paint inputs. A width or theme that
    /// differs from the previous frame drops every parked canvas before the
    /// frame paints: the rows were wrapped at another width or styled for
    /// another theme, so no canvas describes this frame.
    pub(crate) fn begin_frame(&mut self, width: usize, theme: Theme) {
        let identity = FrameIdentity { width, theme };
        if self.frame.is_some_and(|previous| previous != identity) {
            self.invalidations = self.invalidations.saturating_add(1);
            self.parked.clear();
            self.parked_rows = 0;
        }
        self.frame = Some(identity);
    }

    /// The canvas parked for `version` at this geometry, or `None` when the
    /// entry's rows must be rendered. A served lookup counts as a hit and marks
    /// the canvas most recently used.
    pub(crate) fn get(
        &mut self,
        version: EntryVersion,
        rows: usize,
        content_offset: usize,
    ) -> Option<&Buffer> {
        let Some(parked) = self.parked.get_mut(&version.id) else {
            self.misses = self.misses.saturating_add(1);
            return None;
        };
        if parked.revision != version.revision
            || parked.rows != rows
            || parked.content_offset != content_offset
        {
            self.misses = self.misses.saturating_add(1);
            return None;
        }
        self.tick = self.tick.saturating_add(1);
        self.hits = self.hits.saturating_add(1);
        parked.last_used = self.tick;
        Some(&parked.canvas)
    }

    /// Park the canvas a miss just rendered, under the entry's id. A canvas the
    /// entry parked under an older revision is replaced, so one entry holds at
    /// most one.
    pub(crate) fn insert(
        &mut self,
        version: EntryVersion,
        rows: usize,
        content_offset: usize,
        canvas: Buffer,
    ) {
        self.tick = self.tick.saturating_add(1);
        let last_used = self.tick;
        let previous = self.parked.insert(
            version.id,
            ParkedCanvas {
                revision: version.revision,
                rows,
                content_offset,
                canvas,
                last_used,
            },
        );
        if let Some(previous) = previous {
            self.parked_rows = self.parked_rows.saturating_sub(previous.rows);
        }
        self.parked_rows = self.parked_rows.saturating_add(rows);
        self.evict_to_bound();
    }

    /// Evict least-recently-used canvases until both bounds hold. A canvas too
    /// large for the row budget on its own is evicted at once, so the bound
    /// holds even when one entry is bigger than the whole cache.
    fn evict_to_bound(&mut self) {
        while self.parked.len() > self.max_entries || self.parked_rows > self.max_rows {
            let Some(id) = self.least_recently_used() else {
                return;
            };
            let Some(removed) = self.parked.remove(&id) else {
                return;
            };
            self.parked_rows = self.parked_rows.saturating_sub(removed.rows);
            self.evicted = self.evicted.saturating_add(1);
        }
    }

    /// The id of the canvas no frame has read for longest.
    fn least_recently_used(&self) -> Option<u64> {
        self.parked
            .iter()
            .min_by_key(|(_, parked)| parked.last_used)
            .map(|(&id, _)| id)
    }
}

// endregion: --- Methods

// region:    --- Accessors

impl RowCache {
    /// Lifetime count of lookups served from the cache: the entries a frame did
    /// not have to re-project.
    pub(crate) fn hits(&self) -> usize {
        self.hits
    }

    /// Lifetime count of lookups that re-rendered and parked their canvas.
    pub(crate) fn misses(&self) -> usize {
        self.misses
    }

    /// Lifetime count of canvases dropped by the bounds.
    pub(crate) fn evicted_total(&self) -> usize {
        self.evicted
    }

    /// Lifetime count of wholesale drops: one per width or theme change.
    pub(crate) fn invalidations(&self) -> usize {
        self.invalidations
    }

    /// Parked canvases right now.
    pub(crate) fn parked_len(&self) -> usize {
        self.parked.len()
    }

    /// Parked rows right now — what the row budget measures.
    pub(crate) fn parked_rows(&self) -> usize {
        self.parked_rows
    }
}

// endregion: --- Accessors

// region:    --- Support

/// The default bound on parked canvases: a viewport shows far fewer entries
/// than this, and a scrolled-back window refills on the frames that follow.
const MAX_PARKED_ENTRIES: usize = 64;

/// The default bound on parked rows. A canvas costs about 40 bytes per cell, so
/// at a 120-column pane this keeps the cache near five megabytes — enough for
/// the whole visible window at any realistic height.
const MAX_PARKED_ROWS: usize = 1024;

// endregion: --- Support
