//! What a shared UI action does to this surface: the panel and picker requests
//! the fold queues and the caller applies.

use agnt_runtime::protocol::contracts::SharedUiAction;

use crate::rendering::theme::ThemeName;
use crate::state::picker::{ActivePicker, PickerOption, PickerPayload};

use super::AppState;
use super::types::InfoPanel;

impl AppState {
    /// Apply one shared UI action to this surface.
    ///
    /// The action is a panel request, and the surface owns what a panel looks
    /// like: nothing here is published anywhere.
    pub(crate) fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        match action {
            SharedUiAction::Help => {
                self.open_info_panel(InfoPanel::Help);
                true
            }
            SharedUiAction::Status => {
                self.open_info_panel(InfoPanel::Status);
                true
            }
            SharedUiAction::Mcps => {
                self.open_info_panel(InfoPanel::Mcps);
                true
            }
            SharedUiAction::Models => {
                self.info_panel = None;
                self.picker.open(ActivePicker::Model);
                self.picker
                    .position_selection_on(&self.status.identity.active_model_identity);
                self.ensure_invariants();
                true
            }
            SharedUiAction::Agents => {
                self.close_info_panel();
                self.picker.open(ActivePicker::Agent);
                self.pending_agent_picker_refresh = true;
                self.ensure_invariants();
                true
            }
            SharedUiAction::Sessions => {
                self.close_info_panel();
                self.picker.open(ActivePicker::Session);
                self.pending_session_picker_refresh = true;
                self.ensure_invariants();
                true
            }
            SharedUiAction::Themes => {
                let options = ThemeName::all()
                    .into_iter()
                    .map(|name| {
                        let display_name = match name {
                            ThemeName::CatppuccinMocha => "Catppuccin Mocha".to_string(),
                            ThemeName::CatppuccinLatte => "Catppuccin Latte".to_string(),
                            ThemeName::CatppuccinFrappe => "Catppuccin Frappe".to_string(),
                            ThemeName::CatppuccinMacchiato => "Catppuccin Macchiato".to_string(),
                        };
                        PickerOption {
                            id: format!("{name:?}"),
                            display: display_name.clone(),
                            search_text: display_name.clone(),
                            section: None,
                            sort_key: Vec::new(),
                            payload: PickerPayload::Theme,
                        }
                    })
                    .collect();
                self.set_picker_options(ActivePicker::Theme, options);
                self.close_info_panel();
                self.picker.open(ActivePicker::Theme);
                // Highlight the active theme row.
                self.picker
                    .position_selection_on(&format!("{:?}", self.theme_name));
                self.ensure_invariants();
                true
            }
            SharedUiAction::Skills => {
                self.open_info_panel(InfoPanel::Skills);
                true
            }
        }
    }
}
