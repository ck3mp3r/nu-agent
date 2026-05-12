use nu_protocol::{Span, Value};

use crate::agent::ui::tui::state::ModelPickerOption;
use crate::agent::{
    protocol::{
        contracts::{
            InteractiveUi, McpToggleRequest, McpUsabilityState, ProgressUi, SharedUiAction,
            UiMessageSnapshot,
        },
        event::{PermissionDecisionSubmission, UiEvent},
    },
    ui::{
        renderer::UiRenderer,
        tui::runtime::{HybridTerminalEvents, TuiRuntimeRenderer},
    },
};

pub(crate) struct StderrProgressUi<R>
where
    R: UiRenderer,
{
    renderer: R,
}

impl<R> StderrProgressUi<R>
where
    R: UiRenderer,
{
    pub fn new(renderer: R) -> Self {
        Self { renderer }
    }
}

impl<R> ProgressUi for StderrProgressUi<R>
where
    R: UiRenderer,
{
    fn emit(&mut self, event: &UiEvent) {
        self.renderer.emit(event);
    }

    fn flush(&mut self) {
        self.renderer.flush();
    }

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

pub(crate) struct TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    renderer: TuiRuntimeRenderer<R, HybridTerminalEvents>,
}

impl<R> TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    pub fn new(renderer: TuiRuntimeRenderer<R, HybridTerminalEvents>) -> Self {
        Self { renderer }
    }

    pub fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.renderer
            .set_active_model_identity(active_model_identity);
    }

    pub fn set_mcp_lifecycle_projection(
        &mut self,
        projection: Vec<crate::tools::mcp::runtime::McpServerLifecycle>,
    ) {
        self.renderer.set_mcp_lifecycle_projection(projection);
    }

    pub fn set_skills_projection(
        &mut self,
        skills: Vec<crate::agent::protocol::skills::DiscoverableSkill>,
    ) {
        self.renderer.set_skills_projection(skills);
    }

    pub fn mark_skills_discovery_failed(&mut self) {
        self.renderer.mark_skills_discovery_failed();
    }

    pub fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.renderer.set_llm_visible_mcp_tool_count(count);
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.renderer.set_context_window_max_tokens(max_tokens);
    }

    pub fn set_model_picker_options(&mut self, options: Vec<ModelPickerOption>) {
        self.renderer.set_model_picker_options(options);
    }

    pub fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<std::path::PathBuf>) {
        self.renderer.set_repo_branch_caller_cwd(caller_cwd);
    }
}

impl<R> ProgressUi for TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    fn emit(&mut self, event: &UiEvent) {
        self.renderer.emit(event);
    }

    fn flush(&mut self) {
        self.renderer.flush();
    }

    fn take_cancel_requested(&self) -> bool {
        self.renderer.take_cancel_requested()
    }

    fn cancellation_value(&self, span: Span) -> Option<Value> {
        Some(Value::nothing(span))
    }
}

impl<R> InteractiveUi for TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    fn pump_once(&mut self) {
        self.renderer.pump_terminal_once();
    }

    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.renderer.take_submitted_prompt()
    }

    fn take_next_model_picker_launch_request(&mut self) -> bool {
        self.renderer.take_next_model_picker_launch_request()
    }

    fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.renderer
            .take_next_mcp_toggle_request()
            .map(|request| McpToggleRequest {
                server_name: request.server_name,
                enable: request.enable,
            })
    }

    fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.renderer.take_next_model_switch_request()
    }

    fn take_next_permission_decision_submission(&mut self) -> Option<PermissionDecisionSubmission> {
        self.renderer.take_next_permission_decision_submission()
    }

    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        let mapped = match state {
            McpUsabilityState::Enabled => {
                crate::agent::ui::tui::state::McpServerUsabilityState::Enabled
            }
            McpUsabilityState::Disabled => {
                crate::agent::ui::tui::state::McpServerUsabilityState::Disabled
            }
            McpUsabilityState::Failed => {
                crate::agent::ui::tui::state::McpServerUsabilityState::Failed
            }
        };
        let _ = self.renderer.set_mcp_server_state(server_name, mapped);
    }

    fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) {
        let mapped = match state {
            McpUsabilityState::Enabled => {
                crate::agent::ui::tui::state::McpServerUsabilityState::Enabled
            }
            McpUsabilityState::Disabled => {
                crate::agent::ui::tui::state::McpServerUsabilityState::Disabled
            }
            McpUsabilityState::Failed => {
                crate::agent::ui::tui::state::McpServerUsabilityState::Failed
            }
        };
        let _ = self.renderer.set_mcp_server_state_with_details(
            server_name,
            mapped,
            reason,
            llm_visible_mcp_tool_count,
        );
    }

    fn quit_requested(&self) -> bool {
        self.renderer.quit_requested()
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        self.renderer.execute_shared_ui_action(action)
    }

    fn set_active_model_identity(&mut self, active_model_identity: &str) {
        self.renderer
            .set_active_model_identity(active_model_identity.to_string());
    }

    fn fatal_error(&self) -> Option<&str> {
        self.renderer.fatal_error()
    }

    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
        self.renderer.hydrate_transcript_from_messages(messages);
    }
}
