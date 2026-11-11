//! tool-event reducer.

use std::collections::{HashMap, VecDeque};

use agnt_render::tool_view::ToolView;
use agnt_render::transcript::entry::{ToolEntry, ToolKind, TranscriptEntry};
use agnt_render::transcript::ir::Display;
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::protocol::events::{ToolCall, ToolCallId, ToolOutcome};
use agnt_runtime::session::tool_status::ToolStatus;

use super::transcript_store::TranscriptStore;
use super::{ToolCallLine, ToolCallStatus};

/// Tool-domain state extracted from `AppState`: the tool-call rows tracked by
/// key, the active (in-progress) call ids per key, and the next call id.
///
/// Completion de-duplication is structural: the preview and the completion are
/// written to the same transcript entry by index, so a finished call cannot gain
/// a second block. The `ToolCompleted` event carries no display — the drawer
/// derives the completion body from the call and the handler's own payload.
#[derive(Debug, Clone)]
pub struct ToolState {
    pub(crate) calls: Vec<ToolCallLine>,
    pub(crate) active_ids_by_key: HashMap<String, VecDeque<u64>>,
    next_call_id: u64,
    /// Whether a replay batch is folding. A switch publishes the switched-to
    /// session's whole log; the calls it carries have no gate record, so each
    /// derives its card from the call itself. The first live turn after the
    /// batch ends that, and the gate returns to being the only live card source.
    replay_open: bool,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            active_ids_by_key: HashMap::new(),
            next_call_id: 1,
            replay_open: false,
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
        let block_index = self.finish_tool_call(store, call.id(), Some(outcome.status));

        // The drawer derives the completion body from the call and its result
        // data; nothing about the display travels on the event.
        if let Some(display) = call.result_body(Some(&outcome.text))
            && let Some(block_index) = block_index
        {
            store.set_tool_result_at(block_index, display);
        }

        true
    }

    /// Open a replay batch: the switch marker has folded, and every call the
    /// replayed log carries derives its card from the call itself, because a
    /// replayed call is accompanied by no gate record.
    pub(crate) fn open_replay(&mut self) {
        self.replay_open = true;
    }

    /// Close the replay batch: the first live turn after it derives its cards
    /// from gate records alone.
    pub(crate) fn close_replay(&mut self) {
        self.replay_open = false;
    }

    /// Derive a replayed call's pre-run card while a replay batch is folding.
    ///
    /// No producer content is supplied: only the gate computes a diff, and a
    /// replayed call has no gate record. A call outside a replay batch derives
    /// no card here — the gate is the live card's only source.
    ///
    /// Returns whether a card was set.
    pub(crate) fn derive_replayed_preview(
        &mut self,
        store: &mut TranscriptStore,
        call: &ToolCall,
    ) -> bool {
        if !self.replay_open {
            return false;
        }
        let Some(display) = call.preview_body(None) else {
            return false;
        };
        self.set_tool_preview(store, call.id(), &display)
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
        .start_tool_call(call.id(), block_index);
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
            .find(|tool| tool.key() == id)
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
            .find(|call| {
                call.status == ToolCallStatus::InProgress && call.key().as_str() == tool_key
            })
            .map(|call| call.key().clone())
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
