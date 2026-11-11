//! The picker container's state machine: which kind is open, what the active
//! one holds, and what a key does to it.

use crate::interaction::input::Key;
use crate::state::CommandPaletteAction;

use super::types::{
    ActivePicker, PickerContainer, PickerEntry, PickerKeyOutcome, PickerOption, PickerPayload,
    PickerRenderKind, PickerState, SubmitAction, SwitchRequest,
};

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

    pub(super) fn index_of(&self, kind: ActivePicker) -> usize {
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

    /// Positions the active picker's selection on the option whose id matches
    /// `id`. No-op when no option matches or no entry is active.
    pub fn position_selection_on(&mut self, id: &str) {
        let Some(state) = self.active_state_mut() else {
            return;
        };
        if let Some(idx) = state.options.iter().position(|opt| opt.id == id) {
            state.selection = idx;
        }
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

    /// The label of the selected inline-slash option, when the active picker has
    /// one. The accept path submits this text, so the picker owns the lookup.
    pub fn selected_slash_label(&self) -> Option<String> {
        let opt = self.active_state()?.selected()?;
        match &opt.payload {
            PickerPayload::Slash(command) => Some(command.label().to_string()),
            _ => None,
        }
    }

    pub fn handle_key(&mut self, key: Key) -> PickerKeyOutcome {
        let Some(entry) = self.active_entry_mut() else {
            return PickerKeyOutcome::Pass;
        };
        match entry.kind {
            ActivePicker::CommandPalette => match key {
                Key::CtrlC => PickerKeyOutcome::Quit,
                other => Self::query_picker(entry, other, true),
            },
            ActivePicker::Model
            | ActivePicker::Agent
            | ActivePicker::Session
            | ActivePicker::Theme => Self::query_picker(entry, key, true),
            ActivePicker::InlineSlash => match key {
                Key::Enter | Key::Tab => PickerKeyOutcome::Submit(SubmitAction::SlashAccept),
                Key::Esc => {
                    entry.state.close();
                    PickerKeyOutcome::Consumed(true)
                }
                Key::Up | Key::CtrlP | Key::CtrlK => {
                    entry.state.move_up();
                    PickerKeyOutcome::Consumed(true)
                }
                Key::Down | Key::CtrlN | Key::CtrlJ => {
                    entry.state.move_down();
                    PickerKeyOutcome::Consumed(true)
                }
                Key::PageUp | Key::CtrlU => {
                    entry.state.move_page_up();
                    PickerKeyOutcome::Consumed(true)
                }
                Key::PageDown | Key::CtrlD => {
                    entry.state.move_page_down();
                    PickerKeyOutcome::Consumed(true)
                }
                _ => PickerKeyOutcome::Pass,
            },
            ActivePicker::None => PickerKeyOutcome::Pass,
        }
    }

    fn query_picker(entry: &mut PickerEntry, key: Key, edit_query: bool) -> PickerKeyOutcome {
        match key {
            Key::Esc => {
                entry.state.close();
                PickerKeyOutcome::Consumed(true)
            }
            Key::Enter => {
                let Some(resolved) = Self::resolve_submit(entry) else {
                    // No selection: Enter dismisses the picker without dispatching
                    // the entry's placeholder submit action.
                    entry.state.close();
                    return PickerKeyOutcome::Consumed(true);
                };
                PickerKeyOutcome::Submit(resolved)
            }
            Key::Up | Key::CtrlP | Key::CtrlK => {
                entry.state.move_up();
                PickerKeyOutcome::Consumed(true)
            }
            Key::Down | Key::CtrlN | Key::CtrlJ => {
                entry.state.move_down();
                PickerKeyOutcome::Consumed(true)
            }
            Key::PageUp | Key::CtrlU => {
                entry.state.move_page_up();
                PickerKeyOutcome::Consumed(true)
            }
            Key::PageDown | Key::CtrlD => {
                entry.state.move_page_down();
                PickerKeyOutcome::Consumed(true)
            }
            Key::Backspace if edit_query => {
                entry.state.backspace_query_char();
                PickerKeyOutcome::Consumed(true)
            }
            Key::Char(ch) if edit_query => {
                entry.state.append_query_char(ch);
                PickerKeyOutcome::Consumed(true)
            }
            _ => PickerKeyOutcome::Consumed(true),
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
