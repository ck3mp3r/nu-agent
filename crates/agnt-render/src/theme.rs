//! Backend-neutral theme: semantic surfaces and colors, no terminal library.
//!
//! Each backend maps these onto its own style vocabulary: the TUI onto
//! terminal styles, the TTY onto ANSI escapes, the plain backend onto nothing.

// region:    --- Types

/// An RGB color. Backend-neutral on purpose: the render layer never names a
/// terminal library's color type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// The markdown element colors: headings, links, and the code-span syntax
/// palette. One record, because every markdown surface resolves through one
/// vocabulary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Markdown {
    /// Heading colors by level: index 0 is the generic heading style, indices
    /// 1..=6 the per-level styles.
    pub heading: [Color; 7],
    pub link: Color,
    pub link_url: Color,
    pub quote: Color,
    pub list_marker: Color,
    /// Emphasis `MdBold`/`MdItalic`/`MdBoldItalic`; the arms add the weight.
    pub emphasis: Color,
    pub inline_code: Color,
    pub code_keyword: Color,
    pub code_type: Color,
    pub code_function: Color,
    pub code_variable: Color,
    pub code_constant: Color,
    pub code_string: Color,
    pub code_number: Color,
    pub code_operator: Color,
    pub code_punctuation: Color,
    pub code_comment: Color,
}

/// Semantic surfaces a backend maps to its own styles. The default is a black
/// monochrome theme; backends that paint color take a resolved theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Theme {
    /// Pane background.
    pub base: Color,
    /// Body text.
    pub text: Color,
    /// Secondary text: meta, labels, muted spans.
    pub muted: Color,
    /// Idle marks: queued and cancelled status, the muted uncrossed glyph.
    pub idle: Color,
    /// Accent: focus, the tool lane marker.
    pub accent: Color,
    /// The user lane's marker accent.
    pub user_accent: Color,
    /// Running status mark.
    pub running: Color,
    /// Success text: the done mark and added diff glyphs.
    pub success: Color,
    /// Failure text: the failed mark and removed diff glyphs.
    pub error: Color,
    /// User-turn surface.
    pub user_surface: Color,
    /// Code and diff surface.
    pub code_surface: Color,
    /// Notice text.
    pub notice: Color,
    /// Banner text.
    pub banner: Color,
    /// Added-line diff surface.
    pub diff_add_bg: Color,
    /// Removed-line diff surface.
    pub diff_remove_bg: Color,
    /// Markdown element colors.
    pub markdown: Markdown,
}

// endregion: --- Types
