use ratatui::style::{Color, Modifier, Style};

const CTP_MOCHA_RED: Color = Color::Rgb(243, 139, 168);
const CTP_MOCHA_YELLOW: Color = Color::Rgb(249, 226, 175);
const CTP_MOCHA_GREEN: Color = Color::Rgb(166, 227, 161);
const CTP_MOCHA_BLUE: Color = Color::Rgb(137, 180, 250);
const CTP_MOCHA_SAPPHIRE: Color = Color::Rgb(116, 199, 236);
const CTP_MOCHA_LAVENDER: Color = Color::Rgb(180, 190, 254);
const CTP_MOCHA_MAUVE: Color = Color::Rgb(203, 166, 247);
const CTP_MOCHA_PEACH: Color = Color::Rgb(250, 179, 135);
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
    pub role_compaction: Style,
    pub lane_prefix_compaction: Style,
    pub row_compaction: Style,
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
    pub syntax_keyword: Style,
    pub syntax_type: Style,
    pub syntax_function: Style,
    pub syntax_variable: Style,
    pub syntax_constant: Style,
    pub syntax_string: Style,
    pub syntax_number: Style,
    pub syntax_operator: Style,
    pub syntax_punctuation: Style,
    pub syntax_comment: Style,
}

fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

fn fg_dim(color: Color) -> Style {
    fg(color).add_modifier(Modifier::DIM)
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
            focus: fg(CTP_MOCHA_SAPPHIRE),
            subtle_meta: fg(CTP_MOCHA_OVERLAY1),
            input_prompt: fg(CTP_MOCHA_BLUE),
            selection_bg: Style::default().bg(CTP_MOCHA_SURFACE1),
            cancelled_modifier: Modifier::CROSSED_OUT,
            role_user: fg(CTP_MOCHA_BLUE),
            role_assistant: fg(CTP_MOCHA_LAVENDER),
            role_system: fg(CTP_MOCHA_YELLOW),
            role_compaction: fg(CTP_MOCHA_OVERLAY1),
            lane_prefix_compaction: fg_dim(CTP_MOCHA_OVERLAY1),
            row_compaction: Style::default(),
            role_tool: fg(CTP_MOCHA_MAUVE),
            role_separator: fg(CTP_MOCHA_OVERLAY0),
            lane_prefix_user: fg_dim(CTP_MOCHA_BLUE),
            lane_prefix_assistant: fg_dim(CTP_MOCHA_LAVENDER),
            lane_prefix_tool: fg_dim(CTP_MOCHA_MAUVE),
            lane_prefix_system: fg_dim(CTP_MOCHA_YELLOW),
            row_user: Style::default().bg(CTP_MOCHA_SURFACE0),
            row_assistant: Style::default(),
            row_tool: Style::default(),
            row_system: Style::default(),
            tool_meta: fg_dim(CTP_MOCHA_OVERLAY1),
            status_queued: fg(CTP_MOCHA_OVERLAY0),
            status_running: fg(CTP_MOCHA_SAPPHIRE),
            status_done: fg(CTP_MOCHA_GREEN),
            status_failed: fg(CTP_MOCHA_RED),
            status_cancelled: fg(CTP_MOCHA_OVERLAY0),
            inline_code: fg_dim(CTP_MOCHA_YELLOW),
            syntax_keyword: fg(CTP_MOCHA_MAUVE),
            syntax_type: fg(CTP_MOCHA_YELLOW),
            syntax_function: fg(CTP_MOCHA_BLUE),
            syntax_variable: fg(CTP_MOCHA_LAVENDER),
            syntax_constant: fg(CTP_MOCHA_RED),
            syntax_string: fg(CTP_MOCHA_GREEN),
            syntax_number: fg(CTP_MOCHA_PEACH),
            syntax_operator: fg(CTP_MOCHA_SAPPHIRE),
            syntax_punctuation: fg(CTP_MOCHA_OVERLAY1),
            syntax_comment: fg_dim(CTP_MOCHA_OVERLAY0),
        }
    }
}
