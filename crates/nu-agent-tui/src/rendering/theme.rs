use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatppuccinPalette {
    pub rosewater: Color,
    pub flamingo: Color,
    pub pink: Color,
    pub mauve: Color,
    pub red: Color,
    pub maroon: Color,
    pub peach: Color,
    pub yellow: Color,
    pub green: Color,
    pub teal: Color,
    pub sky: Color,
    pub sapphire: Color,
    pub blue: Color,
    pub lavender: Color,
    pub text: Color,
    pub subtext0: Color,
    pub subtext1: Color,
    pub overlay0: Color,
    pub overlay1: Color,
    pub overlay2: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub surface2: Color,
    pub base: Color,
    pub mantle: Color,
    pub crust: Color,
}

impl CatppuccinPalette {
    pub const fn mocha() -> Self {
        Self {
            rosewater: Color::Rgb(245, 224, 220),
            flamingo: Color::Rgb(242, 205, 205),
            pink: Color::Rgb(245, 194, 231),
            mauve: Color::Rgb(203, 166, 247),
            red: Color::Rgb(243, 139, 168),
            maroon: Color::Rgb(235, 160, 172),
            peach: Color::Rgb(250, 179, 135),
            yellow: Color::Rgb(249, 226, 175),
            green: Color::Rgb(166, 227, 161),
            teal: Color::Rgb(148, 226, 213),
            sky: Color::Rgb(137, 220, 235),
            sapphire: Color::Rgb(116, 199, 236),
            blue: Color::Rgb(137, 180, 250),
            lavender: Color::Rgb(180, 190, 254),
            text: Color::Rgb(205, 214, 244),
            subtext0: Color::Rgb(166, 173, 200),
            subtext1: Color::Rgb(186, 194, 222),
            overlay0: Color::Rgb(108, 112, 134),
            overlay1: Color::Rgb(127, 132, 156),
            overlay2: Color::Rgb(147, 153, 178),
            surface0: Color::Rgb(49, 50, 68),
            surface1: Color::Rgb(69, 71, 90),
            surface2: Color::Rgb(88, 91, 112),
            base: Color::Rgb(30, 30, 46),
            mantle: Color::Rgb(24, 24, 37),
            crust: Color::Rgb(17, 17, 27),
        }
    }

    pub const fn latte() -> Self {
        Self {
            rosewater: Color::Rgb(220, 138, 120),
            flamingo: Color::Rgb(221, 120, 120),
            pink: Color::Rgb(234, 118, 203),
            mauve: Color::Rgb(136, 57, 239),
            red: Color::Rgb(210, 15, 57),
            maroon: Color::Rgb(230, 69, 83),
            peach: Color::Rgb(254, 100, 11),
            yellow: Color::Rgb(223, 142, 29),
            green: Color::Rgb(64, 160, 43),
            teal: Color::Rgb(23, 146, 153),
            sky: Color::Rgb(4, 165, 229),
            sapphire: Color::Rgb(32, 159, 181),
            blue: Color::Rgb(30, 102, 245),
            lavender: Color::Rgb(114, 135, 253),
            text: Color::Rgb(76, 79, 105),
            subtext0: Color::Rgb(108, 111, 133),
            subtext1: Color::Rgb(92, 95, 119),
            overlay0: Color::Rgb(156, 160, 176),
            overlay1: Color::Rgb(140, 143, 161),
            overlay2: Color::Rgb(124, 127, 147),
            surface0: Color::Rgb(204, 208, 218),
            surface1: Color::Rgb(188, 192, 204),
            surface2: Color::Rgb(172, 176, 190),
            base: Color::Rgb(239, 241, 245),
            mantle: Color::Rgb(230, 233, 239),
            crust: Color::Rgb(220, 224, 232),
        }
    }

    pub const fn frappe() -> Self {
        Self {
            rosewater: Color::Rgb(242, 213, 207),
            flamingo: Color::Rgb(238, 190, 190),
            pink: Color::Rgb(244, 184, 228),
            mauve: Color::Rgb(202, 158, 230),
            red: Color::Rgb(231, 130, 132),
            maroon: Color::Rgb(234, 153, 156),
            peach: Color::Rgb(239, 159, 118),
            yellow: Color::Rgb(229, 200, 144),
            green: Color::Rgb(166, 209, 137),
            teal: Color::Rgb(129, 200, 190),
            sky: Color::Rgb(153, 209, 219),
            sapphire: Color::Rgb(133, 193, 220),
            blue: Color::Rgb(140, 170, 238),
            lavender: Color::Rgb(186, 187, 241),
            text: Color::Rgb(198, 208, 245),
            subtext0: Color::Rgb(165, 173, 206),
            subtext1: Color::Rgb(181, 191, 226),
            overlay0: Color::Rgb(115, 121, 148),
            overlay1: Color::Rgb(131, 139, 167),
            overlay2: Color::Rgb(148, 156, 187),
            surface0: Color::Rgb(65, 69, 89),
            surface1: Color::Rgb(81, 87, 109),
            surface2: Color::Rgb(98, 104, 128),
            base: Color::Rgb(48, 52, 70),
            mantle: Color::Rgb(41, 44, 60),
            crust: Color::Rgb(35, 38, 52),
        }
    }

    pub const fn macchiato() -> Self {
        Self {
            rosewater: Color::Rgb(244, 219, 214),
            flamingo: Color::Rgb(240, 198, 198),
            pink: Color::Rgb(245, 189, 230),
            mauve: Color::Rgb(198, 160, 246),
            red: Color::Rgb(237, 135, 150),
            maroon: Color::Rgb(238, 153, 160),
            peach: Color::Rgb(245, 169, 127),
            yellow: Color::Rgb(238, 212, 159),
            green: Color::Rgb(166, 218, 149),
            teal: Color::Rgb(139, 213, 202),
            sky: Color::Rgb(145, 215, 227),
            sapphire: Color::Rgb(125, 196, 228),
            blue: Color::Rgb(138, 173, 244),
            lavender: Color::Rgb(183, 189, 248),
            text: Color::Rgb(202, 211, 245),
            subtext0: Color::Rgb(165, 173, 203),
            subtext1: Color::Rgb(184, 192, 224),
            overlay0: Color::Rgb(110, 115, 141),
            overlay1: Color::Rgb(128, 135, 162),
            overlay2: Color::Rgb(147, 154, 183),
            surface0: Color::Rgb(54, 58, 79),
            surface1: Color::Rgb(73, 77, 100),
            surface2: Color::Rgb(91, 96, 120),
            base: Color::Rgb(36, 39, 58),
            mantle: Color::Rgb(30, 32, 48),
            crust: Color::Rgb(24, 25, 38),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeName {
    #[default]
    CatppuccinMocha,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
}

impl ThemeName {
    pub fn resolve(&self) -> TuiTheme {
        match self {
            ThemeName::CatppuccinMocha => TuiTheme::catppuccin_mocha(),
            ThemeName::CatppuccinLatte => TuiTheme::catppuccin_latte(),
            ThemeName::CatppuccinFrappe => TuiTheme::catppuccin_frappe(),
            ThemeName::CatppuccinMacchiato => TuiTheme::catppuccin_macchiato(),
        }
    }

    pub fn all() -> [ThemeName; 4] {
        [
            ThemeName::CatppuccinMocha,
            ThemeName::CatppuccinLatte,
            ThemeName::CatppuccinFrappe,
            ThemeName::CatppuccinMacchiato,
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "CatppuccinMocha" | "catppuccin-mocha" => Some(Self::CatppuccinMocha),
            "CatppuccinLatte" | "catppuccin-latte" => Some(Self::CatppuccinLatte),
            "CatppuccinFrappe" | "catppuccin-frappe" => Some(Self::CatppuccinFrappe),
            "CatppuccinMacchiato" | "catppuccin-macchiato" => Some(Self::CatppuccinMacchiato),
            _ => None,
        }
    }

    /// The canonical kebab-case name used in config.toml, the `--theme` flag,
    /// and the persisted preference file.
    pub fn kebab_name(&self) -> &'static str {
        match self {
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::CatppuccinLatte => "catppuccin-latte",
            Self::CatppuccinFrappe => "catppuccin-frappe",
            Self::CatppuccinMacchiato => "catppuccin-macchiato",
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
    pub surface0: Color,
    pub base: Color,
}

fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

fn fg_dim(color: Color) -> Style {
    fg(color).add_modifier(Modifier::DIM)
}

impl TuiTheme {
    pub fn catppuccin_mocha() -> Self {
        Self::from_palette(&CatppuccinPalette::mocha())
    }

    pub fn catppuccin_latte() -> Self {
        Self::from_palette(&CatppuccinPalette::latte())
    }

    pub fn catppuccin_frappe() -> Self {
        Self::from_palette(&CatppuccinPalette::frappe())
    }

    pub fn catppuccin_macchiato() -> Self {
        Self::from_palette(&CatppuccinPalette::macchiato())
    }

    pub fn from_palette(palette: &CatppuccinPalette) -> Self {
        Self {
            focus: fg(palette.sapphire),
            subtle_meta: fg(palette.overlay1),
            input_prompt: fg(palette.blue),
            input_text: fg(palette.text),
            selection_bg: Style::default().bg(palette.surface1),
            cancelled_modifier: Modifier::CROSSED_OUT,
            role_user: fg(palette.lavender),
            role_assistant: fg(palette.lavender),
            role_system: fg(palette.yellow),
            role_compaction: fg(palette.overlay1),
            lane_prefix_compaction: fg_dim(palette.overlay1),
            row_compaction: Style::default(),
            role_tool: fg(palette.mauve),
            role_separator: fg(palette.overlay0),
            lane_prefix_user: fg_dim(palette.blue),
            lane_prefix_assistant: fg_dim(palette.lavender),
            lane_prefix_tool: fg_dim(palette.mauve),
            lane_prefix_system: fg_dim(palette.yellow),
            row_user: fg(palette.text),
            row_user_bg: palette.surface0,
            row_assistant: fg(palette.text),
            row_tool: fg(palette.text),
            row_system: fg(palette.text),
            tool_meta: fg_dim(palette.overlay1),
            status_queued: fg(palette.overlay0),
            status_running: fg(palette.sapphire),
            status_done: fg(palette.green),
            status_failed: fg(palette.red),
            status_cancelled: fg(palette.overlay0),
            inline_code: fg_dim(palette.yellow),
            syntax_keyword: fg(palette.mauve),
            syntax_type: fg(palette.yellow),
            syntax_function: fg(palette.blue),
            syntax_variable: fg(palette.lavender),
            syntax_constant: fg(palette.red),
            syntax_string: fg(palette.green),
            syntax_number: fg(palette.peach),
            syntax_operator: fg(palette.sapphire),
            syntax_punctuation: fg(palette.overlay1),
            syntax_comment: fg_dim(palette.overlay0),
            surface0: palette.surface0,
            base: palette.base,
        }
    }
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self::catppuccin_mocha()
    }
}
