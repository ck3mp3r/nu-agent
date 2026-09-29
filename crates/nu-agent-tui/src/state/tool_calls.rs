use super::{ToolCallLine, ToolCallStatus};
use nu_agent_core::transcript::ir::Block;
use nu_agent_core::transcript::renderer::ItemStatus;
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

    pub(super) fn start_tool_call(&mut self, name: &str, arguments: &str, block_index: usize) {
        let id = self.next_id();
        let key = tool_call_key(name, arguments);

        self.calls.push(ToolCallLine {
            id,
            status: ToolCallStatus::InProgress,
            key: key.clone(),
            block_index: Some(block_index),
        });
        self.active_ids_by_key.entry(key).or_default().push_back(id);
    }

    /// Record an already-finished (hydrated) tool call. The entry is terminal
    /// from birth: it participates in key lookups (`set_tool_preview`) but
    /// never enters `active_ids_by_key`, so a later live call with the same
    /// key cannot pop it and have its finish land on the wrong block.
    pub(super) fn record_terminal_call(
        &mut self,
        name: &str,
        arguments: &str,
        block_index: usize,
        success: Option<bool>,
    ) {
        let id = self.next_id();
        let key = tool_call_key(name, arguments);

        self.calls.push(ToolCallLine {
            id,
            status: match success {
                Some(true) => ToolCallStatus::Done,
                Some(false) => ToolCallStatus::Failed,
                None => ToolCallStatus::Unknown,
            },
            key,
            block_index: Some(block_index),
        });
    }

    fn next_id(&mut self) -> u64 {
        let id = *self.next_tool_call_id;
        *self.next_tool_call_id = self.next_tool_call_id.saturating_add(1);
        id
    }

    pub(super) fn finish_tool_call(
        &mut self,
        name: &str,
        arguments: &str,
        success: Option<bool>,
        blocks: &mut [Block],
        status: ItemStatus,
    ) {
        let key = tool_call_key(name, arguments);
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

        if let Some(id) = maybe_id
            && let Some(tool) = self.calls.iter_mut().find(|tool| tool.id == id)
        {
            tool.status = match success {
                Some(true) => ToolCallStatus::Done,
                Some(false) => ToolCallStatus::Failed,
                None => ToolCallStatus::Unknown,
            };
            if let Some(block_index) = tool.block_index
                && let Some(block) = blocks.get_mut(block_index)
            {
                block.status = Some(status);
            }
        }
    }
}

fn tool_call_key(name: &str, arguments: &str) -> String {
    format!("{name}\n{arguments}")
}
