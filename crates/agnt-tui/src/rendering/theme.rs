use agnt_core::transcript::ir::DiffTint;
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
            surface0: Color::Rgb(43, 44, 61),
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
            surface0: Color::Rgb(196, 200, 212),
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
            surface0: Color::Rgb(59, 63, 82),
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
            surface0: Color::Rgb(48, 52, 72),
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
    /// Foreground-only bold style behind picker section titles (provider
    /// headers in the model picker, day headers in the session picker). It paints
    /// no background: section titles carry their weight through the bold
    /// modifier alone.
    pub picker_section_header: Style,
    /// Dimmed style for model picker rows whose context window cannot hold the
    /// session's consumed tokens.
    pub picker_row_unavailable: Style,
    pub input_prompt: Style,
    pub input_text: Style,
    pub selection_bg: Style,
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
    /// Line-level diff backgrounds. `Add`/`Remove` are dimmed tints of the
    /// status green/red; `Context` is optional and defaults to no background.
    pub diff_add_bg: Color,
    pub diff_remove_bg: Color,
    pub diff_context_bg: Option<Color>,
    /// Markdown element styles. `md_heading[0]` is the generic heading style
    /// (table header cells); `md_heading[1..=6]` are the per-level heading
    /// styles.
    pub md_heading: [Style; 7],
    pub md_link: Style,
    pub md_link_url: Style,
    pub md_quote: Style,
    pub md_list_marker: Style,
    /// Foreground for markdown emphasis (`MdBold`/`MdItalic`/`MdBoldItalic`);
    /// the arms add the BOLD/ITALIC modifiers on top.
    pub md_emphasis: Style,
}

fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

fn fg_dim(color: Color) -> Style {
    fg(color).add_modifier(Modifier::DIM)
}

/// A dimmed line-level diff tint derived from an accent colour and the theme
/// base. On a dark base the accent is scaled down so the tint stays a
/// saturated, low-luminance background; on a light base the accent is blended
/// toward white so the tint stays light. Either way the syntax foreground
/// colours remain readable on top.
fn dimmed_diff_bg(accent: Color, base: Color) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (accent, base) else {
        return accent;
    };
    let base_luma = (u32::from(br) * 299 + u32::from(bg) * 587 + u32::from(bb) * 114) / 1000;
    if base_luma < 128 {
        let scale = |channel: u8| ((u32::from(channel) * 20) / 100) as u8;
        Color::Rgb(scale(ar), scale(ag), scale(ab))
    } else {
        let blend = |channel: u8| ((u32::from(channel) * 22 + 255 * 78) / 100) as u8;
        Color::Rgb(blend(ar), blend(ag), blend(ab))
    }
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
            picker_section_header: Style::default()
                .fg(palette.subtext0)
                .add_modifier(Modifier::BOLD),
            picker_row_unavailable: fg_dim(palette.overlay1),
            input_prompt: fg(palette.blue),
            input_text: fg(palette.text),
            selection_bg: Style::default().bg(palette.surface1),
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
            inline_code: fg(palette.green),
            syntax_keyword: fg(palette.mauve),
            syntax_type: fg(palette.yellow),
            syntax_function: fg(palette.blue),
            syntax_variable: fg(palette.text),
            syntax_constant: fg(palette.peach),
            syntax_string: fg(palette.green),
            syntax_number: fg(palette.peach),
            syntax_operator: fg(palette.sky),
            syntax_punctuation: fg(palette.overlay2),
            syntax_comment: fg(palette.overlay2).add_modifier(Modifier::ITALIC),
            surface0: palette.surface0,
            base: palette.base,
            diff_add_bg: dimmed_diff_bg(palette.green, palette.base),
            diff_remove_bg: dimmed_diff_bg(palette.red, palette.base),
            diff_context_bg: None,
            md_heading: [
                fg(palette.blue).add_modifier(Modifier::BOLD),
                fg(palette.red).add_modifier(Modifier::BOLD),
                fg(palette.peach).add_modifier(Modifier::BOLD),
                fg(palette.yellow).add_modifier(Modifier::BOLD),
                fg(palette.green).add_modifier(Modifier::BOLD),
                fg(palette.sapphire).add_modifier(Modifier::BOLD),
                fg(palette.lavender).add_modifier(Modifier::BOLD),
            ],
            md_link: fg(palette.lavender),
            md_link_url: fg(palette.blue)
                .add_modifier(Modifier::ITALIC)
                .add_modifier(Modifier::UNDERLINED),
            md_quote: fg(palette.pink),
            md_list_marker: fg(palette.teal),
            md_emphasis: fg(palette.red),
        }
    }
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self::catppuccin_mocha()
    }
}

/// The line-level diff background for a [`DiffTint`], or `None` when the tint
/// carries no background. `Add`/`Remove` always resolve to the theme's dimmed
/// green/red tints; `Context` resolves to `theme.diff_context_bg`, which is
/// `None` by default so context lines keep the surrounding surface.
pub fn diff_tint_to_bg(tint: &DiffTint, theme: &TuiTheme) -> Option<Color> {
    match tint {
        DiffTint::Add => Some(theme.diff_add_bg),
        DiffTint::Remove => Some(theme.diff_remove_bg),
        DiffTint::Context => theme.diff_context_bg,
    }
}

// region:    --- Conversions

/// The backend-neutral theme the render widgets paint with, resolved from this
/// backend's own theme.
///
/// Every surface resolves onto the TUI style that carries it in the pre-wave
/// vocabulary: the bright status colors stay bright (the diff *backgrounds* are
/// row surfaces, never foregrounds), and the markdown/syntax palette rides its
/// own record. A style that carries no RGB foreground falls back to black.
impl From<&TuiTheme> for agnt_render::Theme {
    fn from(theme: &TuiTheme) -> Self {
        let system = neutral_color(&theme.role_system);
        Self {
            base: neutral_color(&Style::default().fg(theme.base)),
            text: neutral_color(&theme.row_assistant),
            muted: neutral_color(&theme.tool_meta),
            idle: neutral_color(&theme.status_queued),
            accent: neutral_color(&theme.role_tool),
            user_accent: neutral_color(&theme.lane_prefix_user),
            running: neutral_color(&theme.status_running),
            success: neutral_color(&theme.status_done),
            error: neutral_color(&theme.status_failed),
            user_surface: neutral_color(&Style::default().fg(theme.row_user_bg)),
            code_surface: neutral_color(&Style::default().fg(theme.surface0)),
            notice: system,
            banner: system,
            diff_add_bg: neutral_color(&Style::default().fg(theme.diff_add_bg)),
            diff_remove_bg: neutral_color(&Style::default().fg(theme.diff_remove_bg)),
            markdown: agnt_render::Markdown {
                heading: [
                    neutral_color(&theme.md_heading[0]),
                    neutral_color(&theme.md_heading[1]),
                    neutral_color(&theme.md_heading[2]),
                    neutral_color(&theme.md_heading[3]),
                    neutral_color(&theme.md_heading[4]),
                    neutral_color(&theme.md_heading[5]),
                    neutral_color(&theme.md_heading[6]),
                ],
                link: neutral_color(&theme.md_link),
                link_url: neutral_color(&theme.md_link_url),
                quote: neutral_color(&theme.md_quote),
                list_marker: neutral_color(&theme.md_list_marker),
                emphasis: neutral_color(&theme.md_emphasis),
                inline_code: neutral_color(&theme.inline_code),
                code_keyword: neutral_color(&theme.syntax_keyword),
                code_type: neutral_color(&theme.syntax_type),
                code_function: neutral_color(&theme.syntax_function),
                code_variable: neutral_color(&theme.syntax_variable),
                code_constant: neutral_color(&theme.syntax_constant),
                code_string: neutral_color(&theme.syntax_string),
                code_number: neutral_color(&theme.syntax_number),
                code_operator: neutral_color(&theme.syntax_operator),
                code_punctuation: neutral_color(&theme.syntax_punctuation),
                code_comment: neutral_color(&theme.syntax_comment),
            },
        }
    }
}

/// One TUI style's foreground as a backend-neutral color. A style with no RGB
/// foreground (only our palette supplies colors) resolves to black.
fn neutral_color(style: &Style) -> agnt_render::Color {
    match style.fg {
        Some(Color::Rgb(r, g, b)) => agnt_render::Color::rgb(r, g, b),
        _ => agnt_render::Color::default(),
    }
}

// endregion: --- Conversions
