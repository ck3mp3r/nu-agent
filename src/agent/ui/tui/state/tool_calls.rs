use super::{ToolCallLine, ToolCallStatus, TranscriptLine};
use std::collections::{HashMap, VecDeque};

pub(super) struct ToolCallBookkeeping<'a> {
    tool_call_items: &'a mut Vec<ToolCallLine>,
    active_tool_ids_by_key: &'a mut HashMap<String, VecDeque<u64>>,
    next_tool_call_id: &'a mut u64,
}

impl<'a> ToolCallBookkeeping<'a> {
    pub(super) fn new(
        tool_call_items: &'a mut Vec<ToolCallLine>,
        active_tool_ids_by_key: &'a mut HashMap<String, VecDeque<u64>>,
        next_tool_call_id: &'a mut u64,
    ) -> Self {
        Self {
            tool_call_items,
            active_tool_ids_by_key,
            next_tool_call_id,
        }
    }

    pub(super) fn start_tool_call(&mut self, transcript_line_index: usize, name: &str, arguments: &str) {
        let id = *self.next_tool_call_id;
        *self.next_tool_call_id = self.next_tool_call_id.saturating_add(1);
        let key = tool_call_key(name, arguments);

        self.tool_call_items.push(ToolCallLine {
            id,
            transcript_line_index,
            status: ToolCallStatus::InProgress,
            key: key.clone(),
        });
        self.active_tool_ids_by_key
            .entry(key)
            .or_default()
            .push_back(id);
    }

    pub(super) fn finish_tool_call(
        &mut self,
        name: &str,
        arguments: &str,
        success: bool,
        transcript_preview: &mut [TranscriptLine],
    ) {
        let key = tool_call_key(name, arguments);
        let maybe_id = self
            .active_tool_ids_by_key
            .get_mut(&key)
            .and_then(|ids| ids.pop_front());
        if self
            .active_tool_ids_by_key
            .get(&key)
            .is_some_and(|ids| ids.is_empty())
        {
            self.active_tool_ids_by_key.remove(&key);
        }

        if let Some(id) = maybe_id
            && let Some(tool) = self.tool_call_items.iter_mut().find(|tool| tool.id == id)
        {
            tool.status = if success {
                ToolCallStatus::Done
            } else {
                ToolCallStatus::Failed
            };

            if let Some(line) = transcript_preview.get_mut(tool.transcript_line_index) {
                line.text = if success {
                    format!("{} · done", line.text)
                } else {
                    format!("{} · failed", line.text)
                };
            }
        }
    }
}

fn tool_call_key(name: &str, arguments: &str) -> String {
    format!("{name}\n{arguments}")
}
