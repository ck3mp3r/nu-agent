use nu_agent_core::protocol::contracts::SharedUiAction;
use nu_agent_core::protocol::picker::{AgentPickerOption, ModelPickerOption};
use nu_agent_core::protocol::slash::SlashCommand;

use super::*;
use crate::interaction::reducer::UserAction;
use crate::state::CommandPaletteAction;

// region:    --- Types

pub trait PickerItem: Clone {
    fn display(&self) -> String;
    fn id(&self) -> String;
    fn matches_query(&self, query: &str) -> bool {
        let q = query.to_ascii_lowercase();
        self.display().to_ascii_lowercase().contains(q.as_str())
            || self.id().to_ascii_lowercase().contains(q.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct PickerOption {
    pub id: String,
    pub display: String,
    pub search_text: String,
    pub payload: PickerPayload,
}

impl PickerItem for PickerOption {
    fn display(&self) -> String {
        self.display.clone()
    }
    fn id(&self) -> String {
        self.id.clone()
    }
    fn matches_query(&self, query: &str) -> bool {
        let q = query.to_ascii_lowercase();
        match &self.payload {
            PickerPayload::Command(_) | PickerPayload::Slash(_) => {
                fuzzy_matches(&q, &self.display.to_ascii_lowercase())
                    || fuzzy_matches(&q, &self.id.to_ascii_lowercase())
                    || fuzzy_matches(&q, &self.search_text.to_ascii_lowercase())
            }
            _ => {
                self.display.to_ascii_lowercase().contains(q.as_str())
                    || self.id.to_ascii_lowercase().contains(q.as_str())
                    || self.search_text.to_ascii_lowercase().contains(q.as_str())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum PickerPayload {
    Model {
        identity: String,
        provider: String,
        provider_display_name: String,
    },
    Agent {
        name: String,
        active: bool,
    },
    Session {
        session_id: String,
        title: Option<String>,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    Theme,
    Command(CommandPaletteAction),
    Slash(SlashCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchRequest {
    Model(String),
    Agent(String),
    Session(String),
    Theme(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitAction {
    Switch(SwitchRequest),
    Launch(ActivePicker),
    Command(CommandPaletteAction),
    SlashAccept,
}

#[derive(Debug, Clone)]
pub struct PickerEntry {
    pub kind: ActivePicker,
    pub state: PickerState<PickerOption>,
    pub edit_query: bool,
    pub submit: SubmitAction,
}

#[derive(Debug, Clone)]
pub struct PickerState<T: PickerItem> {
    pub open: bool,
    pub query: String,
    pub selection: usize,
    pub options: Vec<T>,
}

impl<T: PickerItem> Default for PickerState<T> {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selection: 0,
            options: Vec::new(),
        }
    }
}

impl<T: PickerItem> PickerState<T> {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selection = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selection = 0;
    }
    pub fn filtered(&self) -> Vec<T> {
        if self.query.is_empty() {
            return self.options.clone();
        }
        let q = self.query.to_ascii_lowercase();
        self.options
            .iter()
            .filter(|o| o.matches_query(&q))
            .cloned()
            .collect()
    }
    pub fn selected(&self) -> Option<T> {
        self.filtered().get(self.selection).cloned()
    }
    pub fn move_up(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.selection = 0;
            return;
        }
        self.selection = if self.selection == 0 {
            len.saturating_sub(1)
        } else {
            self.selection.saturating_sub(1)
        };
    }
    pub fn move_down(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.selection = 0;
            return;
        }
        self.selection = (self.selection + 1) % len;
    }
    pub fn append_query_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selection = 0;
    }
    pub fn backspace_query_char(&mut self) {
        self.query.pop();
        self.selection = 0;
    }
    pub fn clamp_selection(&mut self, len: usize) {
        if len == 0 {
            self.selection = 0;
        } else if self.selection >= len {
            self.selection = len.saturating_sub(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePicker {
    CommandPalette,
    Model,
    Agent,
    Session,
    Theme,
    InlineSlash,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerRenderKind {
    CommandPalette,
    Model,
    Agent,
    Session,
    Theme,
    InlineSlash,
}

#[derive(Debug, Clone)]
pub struct PickerContainer {
    pub entries: Vec<PickerEntry>,
}

// endregion: --- Types

impl Default for PickerContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl PickerContainer {
    pub fn new() -> Self {
        let entries = vec![
            PickerEntry {
                kind: ActivePicker::CommandPalette,
                state: PickerState::default(),
                edit_query: true,
                submit: SubmitAction::Command(CommandPaletteAction::Help),
            },
            PickerEntry {
                kind: ActivePicker::Model,
                state: PickerState::default(),
                edit_query: true,
                submit: SubmitAction::Switch(SwitchRequest::Model(String::new())),
            },
            PickerEntry {
                kind: ActivePicker::Agent,
                state: PickerState::default(),
                edit_query: true,
                submit: SubmitAction::Switch(SwitchRequest::Agent(String::new())),
            },
            PickerEntry {
                kind: ActivePicker::Session,
                state: PickerState::default(),
                edit_query: true,
                submit: SubmitAction::Switch(SwitchRequest::Session(String::new())),
            },
            PickerEntry {
                kind: ActivePicker::Theme,
                state: PickerState::default(),
                edit_query: false,
                submit: SubmitAction::Switch(SwitchRequest::Theme(String::new())),
            },
            PickerEntry {
                kind: ActivePicker::InlineSlash,
                state: PickerState::default(),
                edit_query: false,
                submit: SubmitAction::SlashAccept,
            },
        ];
        Self { entries }
    }

    fn index_of(&self, kind: ActivePicker) -> usize {
        match kind {
            ActivePicker::CommandPalette => 0,
            ActivePicker::Model => 1,
            ActivePicker::Agent => 2,
            ActivePicker::Session => 3,
            ActivePicker::Theme => 4,
            ActivePicker::InlineSlash => 5,
            ActivePicker::None => 0,
        }
    }

    pub fn open(&mut self, kind: ActivePicker) -> &mut PickerEntry {
        for e in &mut self.entries {
            e.state.open = false;
        }
        let idx = self.index_of(kind);
        self.entries[idx].state.open = true;
        &mut self.entries[idx]
    }

    pub fn close(&mut self) {
        for e in &mut self.entries {
            e.state.close();
        }
    }

    pub fn active(&self) -> Option<ActivePicker> {
        self.entries.iter().find(|e| e.state.open).map(|e| e.kind)
    }

    pub fn render_kind(&self) -> Option<PickerRenderKind> {
        match self.active() {
            Some(ActivePicker::CommandPalette) => Some(PickerRenderKind::CommandPalette),
            Some(ActivePicker::Model) => Some(PickerRenderKind::Model),
            Some(ActivePicker::Agent) => Some(PickerRenderKind::Agent),
            Some(ActivePicker::Session) => Some(PickerRenderKind::Session),
            Some(ActivePicker::Theme) => Some(PickerRenderKind::Theme),
            Some(ActivePicker::InlineSlash) => Some(PickerRenderKind::InlineSlash),
            Some(ActivePicker::None) | None => None,
        }
    }

    pub fn active_state(&self) -> Option<&PickerState<PickerOption>> {
        self.entries.iter().find(|e| e.state.open).map(|e| &e.state)
    }

    pub fn active_state_mut(&mut self) -> Option<&mut PickerState<PickerOption>> {
        self.entries
            .iter_mut()
            .find(|e| e.state.open)
            .map(|e| &mut e.state)
    }

    pub fn active_entry(&self) -> Option<&PickerEntry> {
        self.entries.iter().find(|e| e.state.open)
    }

    pub fn active_entry_mut(&mut self) -> Option<&mut PickerEntry> {
        self.entries.iter_mut().find(|e| e.state.open)
    }

    pub fn clamp_selections(&mut self) {
        for e in &mut self.entries {
            let len = e.state.filtered().len();
            e.state.clamp_selection(len);
        }
    }

    pub fn handle_action(&mut self, action: UserAction) -> (UserAction, bool) {
        let Some(entry) = self.active_entry_mut() else {
            return (action, false);
        };
        match entry.kind {
            ActivePicker::CommandPalette => match action {
                UserAction::Quit => (UserAction::Quit, true),
                other => Self::query_picker(entry, other, true),
            },
            ActivePicker::Model
            | ActivePicker::Agent
            | ActivePicker::Session
            | ActivePicker::Theme => Self::query_picker(entry, action, true),
            ActivePicker::InlineSlash => match action {
                UserAction::Submit | UserAction::CompleteForward => {
                    (UserAction::PickerSubmit(SubmitAction::SlashAccept), true)
                }
                UserAction::Esc => {
                    entry.state.close();
                    (UserAction::Noop, true)
                }
                UserAction::HistoryUp => {
                    entry.state.move_up();
                    (UserAction::Noop, true)
                }
                UserAction::HistoryDown => {
                    entry.state.move_down();
                    (UserAction::Noop, true)
                }
                other => (other, false),
            },
            ActivePicker::None => (action, false),
        }
    }

    fn query_picker(
        entry: &mut PickerEntry,
        action: UserAction,
        edit_query: bool,
    ) -> (UserAction, bool) {
        match action {
            UserAction::Esc => {
                entry.state.close();
                (UserAction::Noop, true)
            }
            UserAction::Submit => {
                let Some(resolved) = Self::resolve_submit(entry) else {
                    // No selection: Enter dismisses the picker without dispatching
                    // the entry's placeholder submit action.
                    entry.state.close();
                    return (UserAction::Noop, true);
                };
                (UserAction::PickerSubmit(resolved), true)
            }
            UserAction::ScrollLineUp | UserAction::HistoryUp | UserAction::ToggleCommandPalette => {
                entry.state.move_up();
                (UserAction::Noop, true)
            }
            UserAction::ScrollLineDown | UserAction::HistoryDown | UserAction::QueryNext => {
                entry.state.move_down();
                (UserAction::Noop, true)
            }
            UserAction::Backspace if edit_query => {
                entry.state.backspace_query_char();
                (UserAction::Noop, true)
            }
            UserAction::InsertChar(ch) if edit_query => {
                entry.state.append_query_char(ch);
                (UserAction::Noop, true)
            }
            _ => (UserAction::Noop, true),
        }
    }

    fn resolve_submit(entry: &PickerEntry) -> Option<SubmitAction> {
        let opt = entry.state.selected()?;
        let resolved = match &entry.submit {
            SubmitAction::Switch(SwitchRequest::Model(_)) => match &opt.payload {
                PickerPayload::Model { identity, .. } => {
                    SubmitAction::Switch(SwitchRequest::Model(identity.clone()))
                }
                _ => entry.submit.clone(),
            },
            SubmitAction::Switch(SwitchRequest::Agent(_)) => match &opt.payload {
                PickerPayload::Agent { name, .. } => {
                    SubmitAction::Switch(SwitchRequest::Agent(name.clone()))
                }
                _ => entry.submit.clone(),
            },
            SubmitAction::Switch(SwitchRequest::Session(_)) => match &opt.payload {
                PickerPayload::Session { session_id, .. } => {
                    SubmitAction::Switch(SwitchRequest::Session(session_id.clone()))
                }
                _ => entry.submit.clone(),
            },
            SubmitAction::Switch(SwitchRequest::Theme(_)) => {
                SubmitAction::Switch(SwitchRequest::Theme(opt.id.clone()))
            }
            SubmitAction::Command(_) => match &opt.payload {
                PickerPayload::Command(a) => SubmitAction::Command(*a),
                _ => entry.submit.clone(),
            },
            other => other.clone(),
        };
        Some(resolved)
    }
}

// endregion: --- PickerContainer

// region: --- Froms

impl From<ModelPickerOption> for PickerOption {
    fn from(opt: ModelPickerOption) -> Self {
        let search_text = format!(
            "{} {} {}",
            opt.identity, opt.provider, opt.provider_display_name
        );
        Self {
            id: opt.identity.clone(),
            display: opt.display.clone(),
            search_text,
            payload: PickerPayload::Model {
                identity: opt.identity,
                provider: opt.provider,
                provider_display_name: opt.provider_display_name,
            },
        }
    }
}

impl From<AgentPickerOption> for PickerOption {
    fn from(opt: AgentPickerOption) -> Self {
        let search_text = format!("{} {}", opt.name, opt.description.as_deref().unwrap_or(""));
        Self {
            id: opt.name.clone(),
            display: opt.display.clone(),
            search_text,
            payload: PickerPayload::Agent {
                name: opt.name,
                active: opt.active,
            },
        }
    }
}

// endregion: --- Froms

impl AppState {
    pub fn open_info_panel(&mut self, panel: InfoPanel) {
        self.picker.close();
        self.info_panel = Some(panel);
        self.info_panel_scroll = 0;
    }

    pub fn close_info_panel(&mut self) {
        self.info_panel = None;
        self.info_panel_scroll = 0;
    }

    pub fn set_picker_options<T: Into<PickerOption>>(
        &mut self,
        kind: ActivePicker,
        options: Vec<T>,
    ) {
        let mut options: Vec<PickerOption> = options.into_iter().map(Into::into).collect();
        sort_picker_options(kind, &mut options);
        let idx = self.picker.index_of(kind);
        self.picker.entries[idx].state.options = options;
        self.ensure_invariants();
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.status.identity.active_agent_identity = Some(name.to_string());
        for opt in &mut self.picker.entries[2].state.options {
            if let PickerPayload::Agent { name: n, active } = &mut opt.payload {
                *active = n == name;
            }
        }
    }

    pub fn has_agents_to_cycle(&self) -> bool {
        self.status.identity.agent_cycle_names.len() >= 2
    }

    pub fn next_agent_cycle_name(&self) -> Option<String> {
        if !self.has_agents_to_cycle() {
            return None;
        }
        let current = self
            .status
            .identity
            .active_agent_identity
            .as_deref()
            .unwrap_or("");
        let current_idx = self
            .status
            .identity
            .agent_cycle_names
            .iter()
            .position(|n| n == current)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.status.identity.agent_cycle_names.len();
        Some(self.status.identity.agent_cycle_names[next_idx].clone())
    }

    pub fn queue_cycle_agent_request(&mut self) {
        if let Some(next_name) = self.next_agent_cycle_name() {
            self.pending_switch_requests
                .push_back(SwitchRequest::Agent(next_name));
        }
    }

    pub fn queue_switch_request(&mut self, req: SwitchRequest) {
        self.pending_switch_requests.push_back(req);
    }

    pub fn take_next_switch_request(&mut self) -> Option<SwitchRequest> {
        self.pending_switch_requests.pop_front()
    }

    pub fn queue_launch_request(&mut self, action: SharedUiAction) {
        self.pending_launch.push_back(action);
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn take_next_launch_request(&mut self) -> Option<SharedUiAction> {
        self.pending_launch.pop_front()
    }
}

fn sort_picker_options(kind: ActivePicker, options: &mut [PickerOption]) {
    match kind {
        ActivePicker::Model => options.sort_by(|a, b| {
            let (ap, am) = match &a.payload {
                PickerPayload::Model {
                    provider, identity, ..
                } => (provider.as_str(), identity.as_str()),
                _ => ("", ""),
            };
            let (bp, bm) = match &b.payload {
                PickerPayload::Model {
                    provider, identity, ..
                } => (provider.as_str(), identity.as_str()),
                _ => ("", ""),
            };
            ap.to_ascii_lowercase()
                .cmp(&bp.to_ascii_lowercase())
                .then_with(|| am.to_ascii_lowercase().cmp(&bm.to_ascii_lowercase()))
        }),
        ActivePicker::Agent => options.sort_by_key(|a| a.id.to_ascii_lowercase()),
        ActivePicker::Session => options.sort_by_key(|b| match &b.payload {
            PickerPayload::Session { created_at, .. } => std::cmp::Reverse(*created_at),
            _ => std::cmp::Reverse(chrono::DateTime::<chrono::Utc>::MIN_UTC),
        }),
        _ => {}
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
