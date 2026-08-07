use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeName {
    #[default]
    CatppuccinMocha,
    CatppuccinLatte,
}

impl ThemeName {
    pub fn resolve(&self) -> TuiTheme {
        match self {
            ThemeName::CatppuccinMocha => TuiTheme::catppuccin_mocha(),
            ThemeName::CatppuccinLatte => TuiTheme::catppuccin_latte(),
        }
    }

    pub fn all() -> [ThemeName; 2] {
        [ThemeName::CatppuccinMocha, ThemeName::CatppuccinLatte]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "CatppuccinMocha" | "catppuccin-mocha" => Some(Self::CatppuccinMocha),
            "CatppuccinLatte" | "catppuccin-latte" => Some(Self::CatppuccinLatte),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiTheme {
    pub focus: Style,
    pub subtle_meta: Style,
    pub input_prompt: Style,
    pub input_text: Style,
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
    pub row_user_bg: Color,
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

impl TuiTheme {
    pub fn catppuccin_mocha() -> Self {
        const RED: Color = Color::Rgb(243, 139, 168);
        const YELLOW: Color = Color::Rgb(249, 226, 175);
        const GREEN: Color = Color::Rgb(166, 227, 161);
        const BLUE: Color = Color::Rgb(137, 180, 250);
        const SAPPHIRE: Color = Color::Rgb(116, 199, 236);
        const LAVENDER: Color = Color::Rgb(180, 190, 254);
        const MAUVE: Color = Color::Rgb(203, 166, 247);
        const PEACH: Color = Color::Rgb(250, 179, 135);
        const OVERLAY0: Color = Color::Rgb(108, 112, 134);
        const OVERLAY1: Color = Color::Rgb(127, 132, 156);
        #[expect(dead_code)]
        const SURFACE0: Color = Color::Rgb(57, 58, 73);
        const SURFACE1: Color = Color::Rgb(69, 71, 90);
        const USER_BG: Color = Color::Rgb(38, 38, 55);
        Self {
            focus: fg(SAPPHIRE),
            subtle_meta: fg(OVERLAY1),
            input_prompt: fg(BLUE),
            input_text: Style::default(),
            selection_bg: Style::default().bg(SURFACE1),
            cancelled_modifier: Modifier::CROSSED_OUT,
            role_user: fg(LAVENDER),
            role_assistant: fg(LAVENDER),
            role_system: fg(YELLOW),
            role_compaction: fg(OVERLAY1),
            lane_prefix_compaction: fg_dim(OVERLAY1),
            row_compaction: Style::default(),
            role_tool: fg(MAUVE),
            role_separator: fg(OVERLAY0),
            lane_prefix_user: fg_dim(BLUE),
            lane_prefix_assistant: fg_dim(LAVENDER),
            lane_prefix_tool: fg_dim(MAUVE),
            lane_prefix_system: fg_dim(YELLOW),
            row_user: Style::default(),
            row_user_bg: USER_BG,
            row_assistant: Style::default(),
            row_tool: Style::default(),
            row_system: Style::default(),
            tool_meta: fg_dim(OVERLAY1),
            status_queued: fg(OVERLAY0),
            status_running: fg(SAPPHIRE),
            status_done: fg(GREEN),
            status_failed: fg(RED),
            status_cancelled: fg(OVERLAY0),
            inline_code: fg_dim(YELLOW),
            syntax_keyword: fg(MAUVE),
            syntax_type: fg(YELLOW),
            syntax_function: fg(BLUE),
            syntax_variable: fg(LAVENDER),
            syntax_constant: fg(RED),
            syntax_string: fg(GREEN),
            syntax_number: fg(PEACH),
            syntax_operator: fg(SAPPHIRE),
            syntax_punctuation: fg(OVERLAY1),
            syntax_comment: fg_dim(OVERLAY0),
        }
    }

    pub fn catppuccin_latte() -> Self {
        const RED: Color = Color::Rgb(210, 15, 57);
        const YELLOW: Color = Color::Rgb(223, 142, 29);
        const GREEN: Color = Color::Rgb(64, 160, 43);
        const BLUE: Color = Color::Rgb(30, 102, 245);
        const SAPPHIRE: Color = Color::Rgb(32, 159, 181);
        const LAVENDER: Color = Color::Rgb(114, 135, 253);
        const MAUVE: Color = Color::Rgb(136, 57, 239);
        const PEACH: Color = Color::Rgb(254, 100, 11);
        const OVERLAY0: Color = Color::Rgb(156, 160, 176);
        const OVERLAY1: Color = Color::Rgb(140, 143, 161);
        #[expect(dead_code)]
        const SURFACE0: Color = Color::Rgb(204, 208, 218);
        const SURFACE1: Color = Color::Rgb(188, 192, 204);
        const USER_BG: Color = Color::Rgb(228, 230, 236);

        Self {
            focus: fg(SAPPHIRE),
            subtle_meta: fg(OVERLAY1),
            input_prompt: fg(BLUE),
            input_text: Style::default(),
            selection_bg: Style::default().bg(SURFACE1),
            cancelled_modifier: Modifier::CROSSED_OUT,
            role_user: fg(LAVENDER),
            role_assistant: fg(LAVENDER),
            role_system: fg(YELLOW),
            role_compaction: fg(OVERLAY1),
            lane_prefix_compaction: fg_dim(OVERLAY1),
            row_compaction: Style::default(),
            role_tool: fg(MAUVE),
            role_separator: fg(OVERLAY0),
            lane_prefix_user: fg_dim(BLUE),
            lane_prefix_assistant: fg_dim(LAVENDER),
            lane_prefix_tool: fg_dim(MAUVE),
            lane_prefix_system: fg_dim(YELLOW),
            row_user: Style::default(),
            row_user_bg: USER_BG,
            row_assistant: Style::default(),
            row_tool: Style::default(),
            row_system: Style::default(),
            tool_meta: fg_dim(OVERLAY1),
            status_queued: fg(OVERLAY0),
            status_running: fg(SAPPHIRE),
            status_done: fg(GREEN),
            status_failed: fg(RED),
            status_cancelled: fg(OVERLAY0),
            inline_code: fg_dim(YELLOW),
            syntax_keyword: fg(MAUVE),
            syntax_type: fg(YELLOW),
            syntax_function: fg(BLUE),
            syntax_variable: fg(LAVENDER),
            syntax_constant: fg(RED),
            syntax_string: fg(GREEN),
            syntax_number: fg(PEACH),
            syntax_operator: fg(SAPPHIRE),
            syntax_punctuation: fg(OVERLAY1),
            syntax_comment: fg_dim(OVERLAY0),
        }
    }
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self::catppuccin_mocha()
    }
}
