//! tool-event reducer.

use std::collections::{HashMap, VecDeque};

use agnt_core::protocol::event::ToolDisplay;
use agnt_core::protocol::events::{ToolCall, ToolCallId, ToolOutcome};
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::entry::{ToolEntry, ToolKind, TranscriptEntry};
use agnt_core::transcript::ir::Display;
use agnt_core::transcript::renderer::ItemStatus;

use super::transcript_store::TranscriptStore;
use super::{ToolCallLine, ToolCallStatus};

/// Tool-domain state extracted from `AppState`: the tool-call rows tracked by
/// key, the active (in-progress) call ids per key, and the next call id.
///
/// Completion-display de-duplication against a pre-authorize preview is NOT
/// handled here: `HookChain::on_outcome`/`suppress_previewed_display`
/// (agnt-core) already omit `display` from the `Completed` event when a
/// preview was shown, so this layer never sees a duplicate to suppress.
#[derive(Debug, Clone)]
pub struct ToolState {
    pub(crate) calls: Vec<ToolCallLine>,
    pub(crate) active_ids_by_key: HashMap<String, VecDeque<u64>>,
    next_call_id: u64,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            active_ids_by_key: HashMap::new(),
            next_call_id: 1,
        }
    }
}

impl ToolState {
    /// Fold a `ToolStarted` event: push the tool block and record its tracked
    /// call. Returns whether the TUI changed.
    pub fn start_call(&mut self, store: &mut TranscriptStore, call: &ToolCall) -> bool {
        self.start_tool_call(store, call);
        true
    }

    /// Fold a `ToolCompleted` event: close the tracked call and record the
    /// outcome display on the entry's result slot in place. A completion that
    /// carries no display records nothing — the entry keeps its preview, if
    /// any, and the store gains no entry.
    pub fn complete_call(
        &mut self,
        store: &mut TranscriptStore,
        call: &ToolCall,
        outcome: &ToolOutcome,
    ) -> bool {
        let block_index = self.finish_tool_call(store, &call.id, Some(outcome.status));

        if let Some(display) = &outcome.display
            && let Some(block_index) = block_index
        {
            store.set_tool_result_at(block_index, preview_to_display(display));
        }

        true
    }

    pub(crate) fn start_tool_call(&mut self, store: &mut TranscriptStore, call: &ToolCall) {
        let status = ItemStatus::InProgress;
        let entry = ToolEntry {
            kind: ToolKind::from_name(&call.name),
            call: call.clone(),
            preview: None,
            result: None,
            status,
        };
        let evicted = store.push_entry(TranscriptEntry::Tool(entry));
        // Pre-compensate for this push's evictions: the store shifts the
        // streaming cursors itself, and `reduce` shifts the domain bookkeeping
        // by the same count when the event returns. Storing the fresh entry in
        // pre-shift coordinates makes both shifts agree.
        let block_index = store.len().saturating_sub(1) + evicted;

        super::tool_calls::ToolCallBookkeeping::new(
            &mut self.calls,
            &mut self.active_ids_by_key,
            &mut self.next_call_id,
        )
        .start_tool_call(&call.id, block_index);
    }

    /// Attach a pre-execution preview (edit diff, nu code) to the pending tool
    /// entry in place. The entry keeps its identity, so no companion entry is
    /// pushed and the call line never becomes a separate visual unit.
    ///
    /// The preview's lines come from [`Display::project_lines`] at render
    /// time — the same projection the tool block's preview path uses — so the
    /// two cannot drift apart. Returns whether a preview was set.
    pub(crate) fn set_tool_preview(
        &mut self,
        store: &mut TranscriptStore,
        id: &ToolCallId,
        preview: &Display,
    ) -> bool {
        let block_index = self
            .calls
            .iter()
            .rev()
            .find(|tool| &tool.key == id)
            .and_then(|tool| tool.block_index);
        let Some(block_index) = block_index else {
            return false;
        };

        store.set_tool_preview_at(block_index, preview.clone())
    }

    /// Attach a pre-execution preview to the pending call identified by its
    /// exact call key. A permission request carries the key, not a `ToolCall`,
    /// so this path matches the key byte-for-byte instead of rebuilding an id.
    /// Returns whether a preview was set.
    pub(crate) fn set_preview_for_key(
        &mut self,
        store: &mut TranscriptStore,
        tool_key: &str,
        preview: &Display,
    ) -> bool {
        let Some(id) = self
            .calls
            .iter()
            .rev()
            .find(|call| call.status == ToolCallStatus::InProgress && call.key.as_str() == tool_key)
            .map(|call| call.key.clone())
        else {
            return false;
        };
        self.set_tool_preview(store, &id, preview)
    }

    /// Close the tracked call and stamp the entry's terminal status. Returns
    /// the entry index of the finished call, so the caller can record its
    /// completion display in place.
    ///
    /// The status is the variant the completion carries; `None` is a call
    /// nothing finished. The bookkeeping's success verdict is the status's own:
    /// only a success is one.
    pub(crate) fn finish_tool_call(
        &mut self,
        store: &mut TranscriptStore,
        id: &ToolCallId,
        status: Option<ToolStatus>,
    ) -> Option<usize> {
        let item_status = match status {
            Some(ToolStatus::Success) => ItemStatus::Done,
            Some(ToolStatus::Failed) => ItemStatus::Failed,
            Some(ToolStatus::Cancelled) => ItemStatus::Cancelled,
            None => ItemStatus::Unknown,
        };
        let success = status.map(|status| matches!(status, ToolStatus::Success));
        super::tool_calls::ToolCallBookkeeping::new(
            &mut self.calls,
            &mut self.active_ids_by_key,
            &mut self.next_call_id,
        )
        .finish_tool_call(id, success, store.entries_mut(), item_status)
    }
}

/// Convert a protocol event display into the IR display type. The shapes are
/// identical; the IR type keeps the renderer independent of the event layer.
pub(crate) fn preview_to_display(display: &ToolDisplay) -> Display {
    Display {
        title: display.title.clone(),
        sections: display
            .sections
            .iter()
            .map(|section| agnt_core::transcript::ir::DisplaySection {
                label: section.label.clone(),
                kind: section.kind.clone(),
                content: section.content.clone(),
                stats: section.stats.clone(),
            })
            .collect(),
    }
}
