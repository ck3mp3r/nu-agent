//! The session boundary's surface effects: clearing the transcript and showing
//! the startup banner.

use agnt_render::transcript::entry::{BannerEntry, TranscriptEntry};

use super::AppState;
use super::support::STARTUP_LOGOS;

impl AppState {
    pub fn push_startup_logo(&mut self) {
        use rand::RngExt;
        let idx = rand::rng().random_range(0..STARTUP_LOGOS.len());
        let logo = STARTUP_LOGOS[idx];
        let entry = TranscriptEntry::Banner(BannerEntry {
            text: logo.to_string(),
        });
        let evicted = self.transcript.push_entry(entry);
        self.shift_bookkeeping_after_eviction(evicted);
    }

    /// Clears the transcript (store) and resets the scroll position and token
    /// usage display. Cross-domain orchestration: transcript + scroll + status.
    ///
    /// The calls the store tracked index rows this clear drops, so the tool
    /// domain drops them too. The id counter outlives the clear: a dropped id is
    /// never reused by a call the fresh session starts.
    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.tool.calls.clear();
        self.tool.active_ids_by_key.clear();
        self.scroll.scroll_offset = 0;
        self.scroll.following_tail = true;
        self.status.tokens.latest_input_tokens = None;
        self.status.tokens.latest_output_tokens = None;
        self.status.tokens.latest_total_tokens = None;
    }
}
