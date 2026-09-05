use nu_agent_core::bus::WarningEvent;
use nu_agent_core::orchestrator::UiStateEvent;
use nu_agent_core::protocol::contracts::McpUsabilityState;

use super::*;

#[derive(Debug, Clone, Default)]
pub struct StatusState {
    pub(crate) status_line: String,
    pub(crate) tokens: TokenUsage,
    pub(crate) active_agent_identity: Option<String>,
    pub(crate) active_model_identity: String,
    pub(crate) active_persona_icon: Option<String>,
    pub(crate) agent_cycle_names: Vec<String>,
    pub(crate) mcp: McpSkillsState,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub(crate) latest_input_tokens: Option<u64>,
    pub(crate) latest_output_tokens: Option<u64>,
    pub(crate) latest_total_tokens: Option<u64>,
    pub(crate) session_total_tokens: u64,
    pub(crate) context_window_max_tokens: Option<u64>,
}

impl TokenUsage {
    pub fn record_token_usage(&mut self, input_tokens: u64, output_tokens: u64, total_tokens: u64) {
        self.latest_input_tokens = Some(input_tokens);
        self.latest_output_tokens = Some(output_tokens);
        self.latest_total_tokens = Some(total_tokens);
        self.session_total_tokens = self.session_total_tokens.saturating_add(total_tokens);
    }

    pub fn hydrate_latest_total_tokens(&mut self, total_tokens: u64) {
        self.latest_total_tokens = Some(total_tokens);
        if self.session_total_tokens < total_tokens {
            self.session_total_tokens = total_tokens;
        }
    }

    pub fn hydrate_usage(
        &mut self,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_tokens: Option<u64>,
    ) {
        if let Some(input_tokens) = input_tokens {
            self.latest_input_tokens = Some(input_tokens);
        }
        if let Some(output_tokens) = output_tokens {
            self.latest_output_tokens = Some(output_tokens);
        }
        if let Some(total_tokens) = total_tokens {
            self.hydrate_latest_total_tokens(total_tokens);
        }
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.context_window_max_tokens = max_tokens;
    }

    pub fn context_window_max_tokens(&self) -> Option<u64> {
        self.context_window_max_tokens
    }
}

impl StatusState {
    pub fn active_agent_identity(&self) -> Option<&str> {
        self.active_agent_identity.as_deref()
    }

    fn set_active_model_identity(&mut self, identity: &str) {
        self.active_model_identity = identity.to_string();
    }

    fn set_active_persona_icon(&mut self, icon: Option<String>) {
        self.active_persona_icon = icon;
    }

    pub fn reduce_ui_state_event(&mut self, event: UiStateEvent) -> bool {
        match event {
            UiStateEvent::SetActiveModelIdentity(s) => {
                self.set_active_model_identity(&s);
                true
            }
            UiStateEvent::SetActivePersonaIcon(icon) => {
                self.set_active_persona_icon(icon);
                true
            }
            UiStateEvent::SetContextWindowMaxTokens(tokens) => {
                self.tokens.set_context_window_max_tokens(tokens);
                true
            }
            UiStateEvent::SetMcpServerState {
                server,
                state,
                error,
                total,
            } => {
                let mapped = match state {
                    McpUsabilityState::Enabled => McpServerUsabilityState::Enabled,
                    McpUsabilityState::Disabled => McpServerUsabilityState::Disabled,
                    McpUsabilityState::Failed => McpServerUsabilityState::Failed,
                };
                self.mcp
                    .set_mcp_server_state_with_details(&server, mapped, error, total);
                true
            }
            UiStateEvent::SetMcpVisibleToolCount { server, count } => {
                self.mcp
                    .set_mcp_visible_tool_count_by_server_name(&server, count);
                true
            }
            UiStateEvent::SetMcpVisibleToolNames { server, names } => {
                self.mcp
                    .set_mcp_visible_tool_names_by_server_name(&server, names);
                true
            }
            _ => false,
        }
    }

    pub fn reduce_warning_event(&mut self, event: WarningEvent) -> bool {
        match event {
            WarningEvent::Message { message } => {
                self.status_line = message;
                true
            }
            WarningEvent::TurnError { .. } => false,
        }
    }
}
