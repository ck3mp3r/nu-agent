use super::{ToolCallLine, ToolCallStatus};
use agnt_core::protocol::events::ToolCallId;
use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::renderer::ItemStatus;
use std::collections::{HashMap, VecDeque};

pub(super) struct ToolCallBookkeeping<'a> {
    calls: &'a mut Vec<ToolCallLine>,
    active_ids_by_key: &'a mut HashMap<String, VecDeque<u64>>,
    next_tool_call_id: &'a mut u64,
}

impl<'a> ToolCallBookkeeping<'a> {
    pub(super) fn new(
        calls: &'a mut Vec<ToolCallLine>,
        active_ids_by_key: &'a mut HashMap<String, VecDeque<u64>>,
        next_tool_call_id: &'a mut u64,
    ) -> Self {
        Self {
            calls,
            active_ids_by_key,
            next_tool_call_id,
        }
    }

    pub(super) fn start_tool_call(&mut self, key: &ToolCallId, block_index: usize) {
        let id = self.next_id();

        self.calls.push(ToolCallLine {
            id,
            status: ToolCallStatus::InProgress,
            key: key.clone(),
            block_index: Some(block_index),
        });
        self.active_ids_by_key
            .entry(key.as_str().to_string())
            .or_default()
            .push_back(id);
    }

    fn next_id(&mut self) -> u64 {
        let id = *self.next_tool_call_id;
        *self.next_tool_call_id = self.next_tool_call_id.saturating_add(1);
        id
    }

    /// Pop the active call for `key`, stamp its terminal status, and stamp
    /// the tool entry it owns. Returns the entry index when the call was
    /// tracked, so the caller can record the completion display in place.
    pub(super) fn finish_tool_call(
        &mut self,
        key: &ToolCallId,
        success: Option<bool>,
        entries: &mut [TranscriptEntry],
        status: ItemStatus,
    ) -> Option<usize> {
        let key = key.as_str().to_string();
        let maybe_id = self
            .active_ids_by_key
            .get_mut(&key)
            .and_then(|ids| ids.pop_front());
        if self
            .active_ids_by_key
            .get(&key)
            .is_some_and(|ids| ids.is_empty())
        {
            self.active_ids_by_key.remove(&key);
        }

        let id = maybe_id?;
        let tool = self.calls.iter_mut().find(|tool| tool.id == id)?;
        tool.status = match success {
            Some(true) => ToolCallStatus::Done,
            Some(false) => ToolCallStatus::Failed,
            None => ToolCallStatus::Unknown,
        };
        let block_index = tool.block_index;
        if let Some(block_index) = block_index
            && let Some(TranscriptEntry::Tool(entry)) = entries.get_mut(block_index)
        {
            entry.status = status;
        }
        block_index
    }
}
