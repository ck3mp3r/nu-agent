use super::*;

impl AppState {
    pub fn command_palette_actions(&self) -> Vec<CommandPaletteAction> {
        if self.command_palette_query.is_empty() {
            return CommandPaletteAction::PALETTE_ACTIONS.to_vec();
        }

        let query = self.command_palette_query.to_ascii_lowercase();
        CommandPaletteAction::PALETTE_ACTIONS
            .iter()
            .filter(|action| fuzzy_matches(&query, &action.label().to_ascii_lowercase()))
            .copied()
            .collect()
    }

    pub fn command_palette_selected_action(&self) -> Option<CommandPaletteAction> {
        self.command_palette_actions()
            .get(self.command_palette_selection)
            .copied()
    }

    pub fn open_command_palette(&mut self) {
        self.info_panel = None;
        self.command_palette_open = true;
        self.command_palette_query.clear();
        self.command_palette_selection = 0;
    }

    pub fn close_command_palette(&mut self) {
        self.command_palette_open = false;
    }

    pub(crate) fn inline_slash_suggestions(&self) -> &[SlashCommand] {
        &self.inline_slash_commands
    }

    pub(crate) fn inline_slash_selected_command(&self) -> Option<SlashCommand> {
        self.inline_slash_commands
            .get(self.inline_slash_selection)
            .copied()
    }

    pub(crate) fn close_inline_slash_suggestions(&mut self) {
        self.inline_slash_open = false;
        self.inline_slash_selection = 0;
        self.inline_slash_commands.clear();
    }

    pub(crate) fn inline_slash_move_up(&mut self) {
        let len = self.inline_slash_commands.len();
        if len == 0 {
            self.inline_slash_selection = 0;
            return;
        }

        self.inline_slash_selection = if self.inline_slash_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.inline_slash_selection.saturating_sub(1)
        };
    }

    pub(crate) fn inline_slash_move_down(&mut self) {
        let len = self.inline_slash_commands.len();
        if len == 0 {
            self.inline_slash_selection = 0;
            return;
        }

        self.inline_slash_selection = (self.inline_slash_selection + 1) % len;
    }

    pub fn open_info_panel(&mut self, panel: InfoPanel) {
        self.command_palette_open = false;
        self.model_picker_open = false;
        self.info_panel = Some(panel);
        self.info_panel_scroll = 0;
    }

    pub fn close_info_panel(&mut self) {
        self.info_panel = None;
        self.info_panel_scroll = 0;
    }

    pub fn set_model_picker_options(&mut self, mut options: Vec<ModelPickerOption>) {
        options.sort_by(|left, right| {
            left.provider
                .to_ascii_lowercase()
                .cmp(&right.provider.to_ascii_lowercase())
                .then_with(|| {
                    left.model
                        .to_ascii_lowercase()
                        .cmp(&right.model.to_ascii_lowercase())
                })
                .then_with(|| {
                    left.identity
                        .to_ascii_lowercase()
                        .cmp(&right.identity.to_ascii_lowercase())
                })
        });
        self.model_picker_options = options;
        self.ensure_invariants();
    }

    pub fn open_model_picker(&mut self) {
        self.command_palette_open = false;
        self.info_panel = None;
        self.model_picker_open = true;
        self.model_picker_query.clear();
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn close_model_picker(&mut self) {
        self.model_picker_open = false;
        self.model_picker_query.clear();
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn model_picker_close_on_escape(&mut self) {
        self.close_model_picker();
    }

    pub fn model_picker_move_up(&mut self) {
        let len = self.model_picker_filtered_options().len();
        if len == 0 {
            self.model_picker_selection = 0;
            return;
        }

        self.model_picker_selection = if self.model_picker_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.model_picker_selection.saturating_sub(1)
        };
    }

    pub fn model_picker_move_down(&mut self) {
        let len = self.model_picker_filtered_options().len();
        if len == 0 {
            self.model_picker_selection = 0;
            return;
        }

        self.model_picker_selection = (self.model_picker_selection + 1) % len;
    }

    pub fn append_model_picker_query_char(&mut self, ch: char) {
        self.model_picker_query.push(ch);
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn backspace_model_picker_query_char(&mut self) {
        self.model_picker_query.pop();
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn model_picker_filtered_options(&self) -> Vec<ModelPickerOption> {
        if self.model_picker_query.is_empty() {
            return self.model_picker_options.clone();
        }

        let query = self.model_picker_query.to_ascii_lowercase();
        self.model_picker_options
            .iter()
            .filter(|option| {
                option
                    .identity
                    .to_ascii_lowercase()
                    .contains(query.as_str())
                    || option.display.to_ascii_lowercase().contains(query.as_str())
            })
            .cloned()
            .collect()
    }

    pub fn selected_model_picker_option(&self) -> Option<ModelPickerOption> {
        self.model_picker_filtered_options()
            .get(self.model_picker_selection)
            .cloned()
    }

    pub fn queue_selected_model_switch_request(&mut self) -> bool {
        let Some(selected) = self.selected_model_picker_option() else {
            return false;
        };
        self.pending_model_switch_requests
            .push_back(selected.identity.clone());
        true
    }

    pub fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.pending_model_switch_requests.pop_front()
    }

    pub fn queue_model_picker_launch_request(&mut self) {
        self.pending_model_picker_launch_requests =
            self.pending_model_picker_launch_requests.saturating_add(1);
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn take_next_model_picker_launch_request(&mut self) -> bool {
        if self.pending_model_picker_launch_requests == 0 {
            return false;
        }
        self.pending_model_picker_launch_requests =
            self.pending_model_picker_launch_requests.saturating_sub(1);
        true
    }

    pub fn set_agent_picker_options(&mut self, options: Vec<AgentPickerOption>) {
        let mut options = options;
        options.sort_by(|a, b| a.name.cmp(&b.name));
        self.agent_picker_options = options;
        self.ensure_invariants();
    }

    pub fn open_agent_picker(&mut self) {
        self.close_command_palette();
        self.close_info_panel();
        self.agent_picker_open = true;
        self.agent_picker_query.clear();
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn close_agent_picker(&mut self) {
        self.agent_picker_open = false;
        self.agent_picker_query.clear();
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn agent_picker_close_on_escape(&mut self) {
        self.close_agent_picker();
    }

    pub fn agent_picker_move_up(&mut self) {
        let count = self.agent_picker_filtered_options().len();
        if count == 0 {
            return;
        }
        if self.agent_picker_selection == 0 {
            self.agent_picker_selection = count - 1;
        } else {
            self.agent_picker_selection -= 1;
        }
    }

    pub fn agent_picker_move_down(&mut self) {
        let count = self.agent_picker_filtered_options().len();
        if count == 0 {
            return;
        }
        self.agent_picker_selection = (self.agent_picker_selection + 1) % count;
    }

    pub fn append_agent_picker_query_char(&mut self, ch: char) {
        self.agent_picker_query.push(ch);
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn backspace_agent_picker_query_char(&mut self) {
        self.agent_picker_query.pop();
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn agent_picker_filtered_options(&self) -> Vec<AgentPickerOption> {
        if self.agent_picker_query.is_empty() {
            return self.agent_picker_options.clone();
        }
        let query = self.agent_picker_query.to_lowercase();
        self.agent_picker_options
            .iter()
            .filter(|o| {
                o.name.to_lowercase().contains(&query) || o.display.to_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }

    pub fn selected_agent_picker_option(&self) -> Option<AgentPickerOption> {
        let filtered = self.agent_picker_filtered_options();
        filtered.get(self.agent_picker_selection).cloned()
    }

    pub fn queue_selected_agent_switch_request(&mut self) -> bool {
        if let Some(option) = self.selected_agent_picker_option() {
            self.pending_agent_switch_requests.push_back(option.name);
            true
        } else {
            false
        }
    }

    pub fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.pending_agent_switch_requests.pop_front()
    }

    pub fn queue_agent_picker_launch_request(&mut self) {
        self.pending_agent_picker_launch_requests =
            self.pending_agent_picker_launch_requests.saturating_add(1);
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn take_next_agent_picker_launch_request(&mut self) -> bool {
        if self.pending_agent_picker_launch_requests > 0 {
            self.pending_agent_picker_launch_requests -= 1;
            true
        } else {
            false
        }
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.active_agent_identity = Some(name.to_string());
        for option in &mut self.agent_picker_options {
            option.active = option.name == name;
        }
    }

    pub fn active_agent_identity(&self) -> Option<&str> {
        self.active_agent_identity.as_deref()
    }

    pub fn has_agents_to_cycle(&self) -> bool {
        self.agent_cycle_names.len() >= 2
    }

    pub fn next_agent_cycle_name(&self) -> Option<String> {
        if !self.has_agents_to_cycle() {
            return None;
        }
        let current = self.active_agent_identity.as_deref().unwrap_or("");
        let current_idx = self
            .agent_cycle_names
            .iter()
            .position(|n| n == current)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.agent_cycle_names.len();
        Some(self.agent_cycle_names[next_idx].clone())
    }

    pub fn queue_cycle_agent_request(&mut self) {
        if let Some(next_name) = self.next_agent_cycle_name() {
            self.pending_agent_switch_requests.push_back(next_name);
        }
    }

    pub fn command_palette_move_up(&mut self) {
        let len = self.command_palette_actions().len();
        if len == 0 {
            self.command_palette_selection = 0;
            return;
        }

        self.command_palette_selection = if self.command_palette_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.command_palette_selection.saturating_sub(1)
        };
    }

    pub fn command_palette_move_down(&mut self) {
        let len = self.command_palette_actions().len();
        if len == 0 {
            self.command_palette_selection = 0;
            return;
        }

        self.command_palette_selection = (self.command_palette_selection + 1) % len;
    }

    pub fn append_command_palette_query_char(&mut self, ch: char) {
        self.command_palette_query.push(ch);
        self.command_palette_selection = 0;
    }

    pub fn backspace_command_palette_query_char(&mut self) {
        self.command_palette_query.pop();
        self.command_palette_selection = 0;
    }
}

impl AppState {
    pub fn set_session_picker_options(&mut self, options: Vec<SessionPickerOption>) {
        let mut options = options;
        options.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        self.session_picker_options = options;
        self.ensure_invariants();
    }

    pub fn open_session_picker(&mut self) {
        self.close_command_palette();
        self.close_info_panel();
        self.session_picker_open = true;
        self.session_picker_query.clear();
        self.session_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn close_session_picker(&mut self) {
        self.session_picker_open = false;
        self.session_picker_query.clear();
        self.session_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn session_picker_close_on_escape(&mut self) {
        self.close_session_picker();
    }

    pub fn session_picker_move_up(&mut self) {
        let count = self.session_picker_filtered_options().len();
        if count == 0 {
            return;
        }
        if self.session_picker_selection == 0 {
            self.session_picker_selection = count - 1;
        } else {
            self.session_picker_selection -= 1;
        }
    }

    pub fn session_picker_move_down(&mut self) {
        let count = self.session_picker_filtered_options().len();
        if count == 0 {
            return;
        }
        self.session_picker_selection = (self.session_picker_selection + 1) % count;
    }

    pub fn append_session_picker_query_char(&mut self, ch: char) {
        self.session_picker_query.push(ch);
        self.session_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn backspace_session_picker_query_char(&mut self) {
        self.session_picker_query.pop();
        self.session_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn session_picker_filtered_options(&self) -> Vec<&SessionPickerOption> {
        if self.session_picker_query.is_empty() {
            return self.session_picker_options.iter().collect();
        }
        let query = self.session_picker_query.to_lowercase();
        self.session_picker_options
            .iter()
            .filter(|o| {
                o.id.to_lowercase().contains(&query)
                    || o.display.to_lowercase().contains(&query)
                    || o.title
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
            })
            .collect()
    }

    pub fn selected_session_picker_option(&self) -> Option<&SessionPickerOption> {
        let filtered = self.session_picker_filtered_options();
        filtered.get(self.session_picker_selection).copied()
    }

    pub fn queue_session_picker_launch_request(&mut self) {
        self.pending_session_picker_launch_requests = self
            .pending_session_picker_launch_requests
            .saturating_add(1);
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn take_next_session_picker_launch_request(&mut self) -> bool {
        if self.pending_session_picker_launch_requests > 0 {
            self.pending_session_picker_launch_requests -= 1;
            true
        } else {
            false
        }
    }

    pub fn queue_session_switch_request(&mut self, session_id: String) {
        self.pending_session_switch_requests.push_back(session_id);
    }

    pub fn take_next_session_switch_request(&mut self) -> Option<String> {
        self.pending_session_switch_requests.pop_front()
    }
}

impl AppState {
    pub fn set_theme_picker_options(&mut self, options: Vec<ThemePickerOption>) {
        self.theme_picker_options = options;
        self.ensure_invariants();
    }

    pub fn open_theme_picker(&mut self) {
        self.close_command_palette();
        self.close_info_panel();
        self.theme_picker_open = true;
        self.theme_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn close_theme_picker(&mut self) {
        self.theme_picker_open = false;
        self.theme_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn theme_picker_close_on_escape(&mut self) {
        self.close_theme_picker();
    }

    pub fn theme_picker_move_up(&mut self) {
        let count = self.theme_picker_options.len();
        if count == 0 {
            return;
        }
        if self.theme_picker_selection == 0 {
            self.theme_picker_selection = count - 1;
        } else {
            self.theme_picker_selection -= 1;
        }
    }

    pub fn theme_picker_move_down(&mut self) {
        let count = self.theme_picker_options.len();
        if count == 0 {
            return;
        }
        self.theme_picker_selection = (self.theme_picker_selection + 1) % count;
    }

    pub fn selected_theme_picker_option(&self) -> Option<&ThemePickerOption> {
        self.theme_picker_options.get(self.theme_picker_selection)
    }

    pub fn queue_theme_picker_launch_request(&mut self) {
        self.pending_theme_picker_launch_requests =
            self.pending_theme_picker_launch_requests.saturating_add(1);
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn take_next_theme_picker_launch_request(&mut self) -> bool {
        if self.pending_theme_picker_launch_requests > 0 {
            self.pending_theme_picker_launch_requests -= 1;
            true
        } else {
            false
        }
    }

    pub fn queue_selected_theme_switch_request(&mut self) -> bool {
        let Some(option) = self.selected_theme_picker_option() else {
            return false;
        };
        self.pending_theme_switch_requests
            .push_back(option.name.clone());
        true
    }

    pub fn take_next_theme_switch_request(&mut self) -> Option<String> {
        self.pending_theme_switch_requests.pop_front()
    }
}

fn fuzzy_matches(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    let mut query_chars = query.chars();
    let mut needle = query_chars.next();
    for ch in candidate.chars() {
        if Some(ch) == needle {
            needle = query_chars.next();
            if needle.is_none() {
                return true;
            }
        }
    }
    false
}
