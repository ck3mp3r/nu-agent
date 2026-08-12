use super::{PromptStatus, QueuedPrompt};
use std::collections::VecDeque;

pub(super) struct PromptQueueLifecycle<'a> {
    prompt_items: &'a mut Vec<QueuedPrompt>,
    pending_prompt_ids: &'a mut VecDeque<u64>,
    active_prompt_id: &'a mut Option<u64>,
    next_prompt_id: &'a mut u64,
}

impl<'a> PromptQueueLifecycle<'a> {
    pub(super) fn new(
        prompt_items: &'a mut Vec<QueuedPrompt>,
        pending_prompt_ids: &'a mut VecDeque<u64>,
        active_prompt_id: &'a mut Option<u64>,
        next_prompt_id: &'a mut u64,
    ) -> Self {
        Self {
            prompt_items,
            pending_prompt_ids,
            active_prompt_id,
            next_prompt_id,
        }
    }

    pub(super) fn enqueue_prompt(&mut self, submitted_text: String) -> u64 {
        let id = *self.next_prompt_id;
        *self.next_prompt_id = self.next_prompt_id.saturating_add(1);

        self.prompt_items.push(QueuedPrompt {
            id,
            prompt_text: submitted_text,
            status: PromptStatus::Queued,
            entry_id: None,
        });
        self.pending_prompt_ids.push_back(id);
        id
    }

    pub(super) fn activate_next_prompt(&mut self) -> Option<u64> {
        if self.active_prompt_id.is_some() {
            return None;
        }

        let next_id = self.pending_prompt_ids.pop_front()?;
        if let Some(prompt) = self
            .prompt_items
            .iter_mut()
            .find(|prompt| prompt.id == next_id)
        {
            prompt.status = PromptStatus::InProgress;
        }
        *self.active_prompt_id = Some(next_id);
        Some(next_id)
    }

    pub(super) fn complete_active_prompt(&mut self) {
        if let Some(active_id) = self.active_prompt_id.take()
            && let Some(prompt) = self
                .prompt_items
                .iter_mut()
                .find(|prompt| prompt.id == active_id)
        {
            prompt.status = PromptStatus::Done;
        }
    }

    pub(super) fn cancel_active_and_pending_prompts(&mut self) {
        if let Some(active_id) = self.active_prompt_id.take()
            && let Some(prompt) = self
                .prompt_items
                .iter_mut()
                .find(|prompt| prompt.id == active_id)
        {
            prompt.status = PromptStatus::Cancelled;
        }

        let pending_ids = self.pending_prompt_ids.drain(..).collect::<Vec<_>>();
        for pending_id in pending_ids {
            if let Some(prompt) = self
                .prompt_items
                .iter_mut()
                .find(|prompt| prompt.id == pending_id)
            {
                prompt.status = PromptStatus::Cancelled;
            }
        }
    }

    pub(super) fn cancel_and_drain_to_texts(&mut self) -> Vec<String> {
        if let Some(active_id) = self.active_prompt_id.take()
            && let Some(prompt) = self.prompt_items.iter_mut().find(|p| p.id == active_id)
        {
            prompt.status = PromptStatus::Cancelled;
        }

        self.pending_prompt_ids
            .drain(..)
            .filter_map(|id| {
                self.prompt_items.iter_mut().find(|p| p.id == id).map(|p| {
                    p.status = PromptStatus::Cancelled;
                    p.prompt_text.clone()
                })
            })
            .collect()
    }

    pub(super) fn coalesce_pending_prompts(&mut self) -> Vec<String> {
        if self.pending_prompt_ids.is_empty() {
            return vec![];
        }

        // FIX: Don't coalesce if there's already an active prompt — wait for it to complete.
        // Without this guard, coalesce overwrites active_prompt_id, then enforce_single_active_invariant
        // re-queues the prompt because two InProgress prompts exist, causing double delivery.
        if self.active_prompt_id.is_some() {
            return vec![];
        }

        let ids: Vec<u64> = self.pending_prompt_ids.drain(..).collect();
        *self.active_prompt_id = ids.first().copied();

        let mut texts = Vec::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            if let Some(p) = self.prompt_items.iter_mut().find(|p| p.id == id) {
                if i == 0 {
                    p.status = PromptStatus::InProgress;
                } else {
                    p.status = PromptStatus::Done;
                }
                texts.push(p.prompt_text.clone());
            }
        }
        texts
    }

    pub(super) fn enforce_single_active_invariant(&mut self) {
        let in_progress_count = self
            .prompt_items
            .iter()
            .filter(|prompt| prompt.status == PromptStatus::InProgress)
            .count();
        if in_progress_count > 1 {
            if let Some(first_in_progress) = self
                .prompt_items
                .iter_mut()
                .find(|prompt| prompt.status == PromptStatus::InProgress)
            {
                *self.active_prompt_id = Some(first_in_progress.id);
            }
            let keep = *self.active_prompt_id;
            for prompt in self.prompt_items.iter_mut() {
                if prompt.status == PromptStatus::InProgress && Some(prompt.id) != keep {
                    prompt.status = PromptStatus::Queued;
                    self.pending_prompt_ids.push_front(prompt.id);
                }
            }
        }

        if let Some(active_id) = *self.active_prompt_id
            && let Some(prompt) = self
                .prompt_items
                .iter_mut()
                .find(|prompt| prompt.id == active_id)
            && prompt.status != PromptStatus::InProgress
        {
            prompt.status = PromptStatus::InProgress;
        }
    }
}
