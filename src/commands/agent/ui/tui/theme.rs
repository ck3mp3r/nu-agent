use ratatui::style::{Color, Modifier, Style};

const CTP_MOCHA_RED: Color = Color::Rgb(243, 139, 168);
const CTP_MOCHA_YELLOW: Color = Color::Rgb(249, 226, 175);
const CTP_MOCHA_GREEN: Color = Color::Rgb(166, 227, 161);
const CTP_MOCHA_BLUE: Color = Color::Rgb(137, 180, 250);
const CTP_MOCHA_SAPPHIRE: Color = Color::Rgb(116, 199, 236);
const CTP_MOCHA_LAVENDER: Color = Color::Rgb(180, 190, 254);
const CTP_MOCHA_MAUVE: Color = Color::Rgb(203, 166, 247);
const CTP_MOCHA_OVERLAY0: Color = Color::Rgb(108, 112, 134);
const CTP_MOCHA_OVERLAY1: Color = Color::Rgb(127, 132, 156);
const CTP_MOCHA_SURFACE0: Color = Color::Rgb(49, 50, 68);
const CTP_MOCHA_SURFACE1: Color = Color::Rgb(69, 71, 90);

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
    pub lane_prefix_user: Style,
    pub lane_prefix_assistant: Style,
    pub lane_prefix_tool: Style,
    pub lane_prefix_system: Style,
    pub row_user: Style,
    pub row_assistant: Style,
    pub row_tool: Style,
    pub row_system: Style,
    pub tool_meta: Style,
    pub status_queued: Style,
    pub status_running: Style,
    pub status_done: Style,
    pub status_failed: Style,
    pub status_cancelled: Style,
    pub inline_code: Style,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
            focus: Style::default().fg(CTP_MOCHA_SAPPHIRE),
            subtle_meta: Style::default().fg(CTP_MOCHA_OVERLAY1),
            input_prompt: Style::default().fg(CTP_MOCHA_BLUE),
            selection_bg: Style::default().bg(CTP_MOCHA_SURFACE1),
            cancelled_modifier: Modifier::CROSSED_OUT,
            role_user: Style::default().fg(CTP_MOCHA_BLUE),
            role_assistant: Style::default().fg(CTP_MOCHA_LAVENDER),
            role_system: Style::default().fg(CTP_MOCHA_YELLOW),
            role_tool: Style::default().fg(CTP_MOCHA_MAUVE),
            role_separator: Style::default().fg(CTP_MOCHA_OVERLAY0),
            lane_prefix_user: Style::default().fg(CTP_MOCHA_BLUE).add_modifier(Modifier::DIM),
            lane_prefix_assistant: Style::default()
                .fg(CTP_MOCHA_LAVENDER)
                .add_modifier(Modifier::DIM),
            lane_prefix_tool: Style::default().fg(CTP_MOCHA_MAUVE).add_modifier(Modifier::DIM),
            lane_prefix_system: Style::default().fg(CTP_MOCHA_YELLOW).add_modifier(Modifier::DIM),
            row_user: Style::default().bg(CTP_MOCHA_SURFACE0),
            row_assistant: Style::default(),
            row_tool: Style::default(),
            row_system: Style::default(),
            tool_meta: Style::default()
                .fg(CTP_MOCHA_OVERLAY1)
                .add_modifier(Modifier::DIM),
            status_queued: Style::default().fg(CTP_MOCHA_OVERLAY0),
            status_running: Style::default().fg(CTP_MOCHA_SAPPHIRE),
            status_done: Style::default().fg(CTP_MOCHA_GREEN),
            status_failed: Style::default().fg(CTP_MOCHA_RED),
            status_cancelled: Style::default().fg(CTP_MOCHA_OVERLAY0),
            inline_code: Style::default().fg(CTP_MOCHA_YELLOW).add_modifier(Modifier::DIM),
        }
    }
}
