use super::{ToolCallLine, ToolCallStatus};
use nu_agent_core::transcript::items::TranscriptEntry;
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

    pub(super) fn start_tool_call(&mut self, name: &str, arguments: &str, entry_id: Option<u64>) {
        let id = *self.next_tool_call_id;
        *self.next_tool_call_id = self.next_tool_call_id.saturating_add(1);
        let key = tool_call_key(name, arguments);

        self.calls.push(ToolCallLine {
            id,
            status: ToolCallStatus::InProgress,
            key: key.clone(),
            entry_id,
        });
        self.active_ids_by_key.entry(key).or_default().push_back(id);
    }

    pub(super) fn finish_tool_call(
        &mut self,
        name: &str,
        arguments: &str,
        success: Option<bool>,
        entries: &mut [TranscriptEntry],
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
            if let Some(entry_id) = tool.entry_id
                && let Some(entry) = entries.iter_mut().rev().find(|entry| entry.id == entry_id)
            {
                entry.status = Some(status);
            }
        }
    }
}

fn tool_call_key(name: &str, arguments: &str) -> String {
    format!("{name}\n{arguments}")
}
