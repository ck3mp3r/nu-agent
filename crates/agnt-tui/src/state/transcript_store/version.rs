//! The entry version ledger: one stable id and one content revision per entry.
//!
//! The rendered-row cache keys on an entry's id and validates its revision, so
//! both facts must survive the entry list's reshuffles: cap eviction drains the
//! ledger alongside the entries, and a survivor keeps the identity it was
//! pushed with. An index is not an identity — it moves when the oldest entries
//! go.

// region:    --- Types

/// One entry's cache identity: `id` is stable for the entry's lifetime, and
/// `revision` moves on every content change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntryVersion {
    pub(crate) id: u64,
    pub(crate) revision: u64,
}

/// One identity per entry, parallel to the entry list.
///
/// A version value is never reused: the counter only grows, so two versions of
/// any two entries never share a revision, and an id the cache still holds can
/// never be handed to a new entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Versions {
    entries: Vec<EntryVersion>,
    /// The next version value. Never reset, not even when the transcript is
    /// cleared: an entry id must stay unique for as long as the cache keyed on
    /// it lives, and that cache outlives one transcript.
    next: u64,
}

// endregion: --- Types

// region:    --- Methods

impl Versions {
    /// Give the entry just pushed a fresh identity.
    pub(super) fn push(&mut self) {
        let version = self.next;
        self.next = self.next.saturating_add(1);
        self.entries.push(EntryVersion {
            id: version,
            revision: version,
        });
    }

    /// Move entry `index`'s content revision. The id stays, so the entry's
    /// parked rows are replaced rather than accumulated.
    pub(super) fn bump(&mut self, index: usize) {
        let Some(version) = self.entries.get_mut(index) else {
            return;
        };
        version.revision = self.next;
        self.next = self.next.saturating_add(1);
    }

    /// Entry `index`'s identity, or `None` for an entry the ledger never saw.
    pub(super) fn get(&self, index: usize) -> Option<EntryVersion> {
        self.entries.get(index).copied()
    }

    /// Drop the oldest `count` identities — the entries they describe are gone.
    pub(super) fn drain(&mut self, count: usize) {
        self.entries.drain(..count.min(self.entries.len()));
    }

    pub(super) fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
    }

    /// Drop the ledger with the entries. The counter keeps counting: a new
    /// transcript must not hand out an id a parked canvas already answers to.
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
}

// endregion: --- Methods
