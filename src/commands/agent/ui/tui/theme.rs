use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct TuiTheme {
    pub focus: Style,
    pub subtle_meta: Style,
    pub input_prompt: Style,
    pub selection_bg: Style,
    pub cancelled_modifier: Modifier,
    pub role_user: Style,
    pub role_assistant: Style,
    pub role_system: Style,
    pub role_tool: Style,
    pub role_separator: Style,
    pub inline_code: Style,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
            focus: Style::default().fg(Color::Cyan),
            subtle_meta: Style::default().fg(Color::DarkGray),
            input_prompt: Style::default().fg(Color::Cyan),
            selection_bg: Style::default().bg(Color::DarkGray),
            cancelled_modifier: Modifier::CROSSED_OUT,
            role_user: Style::default().fg(Color::Cyan),
            role_assistant: Style::default().fg(Color::Green),
            role_system: Style::default().fg(Color::Yellow),
            role_tool: Style::default().fg(Color::Magenta),
            role_separator: Style::default().fg(Color::DarkGray),
            inline_code: Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM),
        }
    }
}
