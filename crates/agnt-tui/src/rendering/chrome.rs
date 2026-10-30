//! The bordered chrome container every non-transcript surface draws.

use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Clear},
};

// region:    --- Types

/// The glyph set a chrome border draws its rules and corners with.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ContainerBorder {
    /// The terminal's default border glyphs.
    #[default]
    Plain,
    /// Rounded corner glyphs.
    Rounded,
}

impl ContainerBorder {
    fn glyphs(self) -> border::Set<'static> {
        match self {
            Self::Plain => border::Set::default(),
            Self::Rounded => border::ROUNDED,
        }
    }
}

/// One bordered chrome container: the six things that vary across the app's
/// chrome surfaces — `clear`, `bg`, `border`, `border_style`, `title`,
/// `title_style`.
#[derive(Debug, Default, Clone)]
pub struct Chrome {
    clear: bool,
    bg: Option<Style>,
    border: ContainerBorder,
    border_style: Style,
    title: Option<Line<'static>>,
    title_style: Option<Style>,
}

// endregion: --- Types

// region:    --- Chrome

impl Chrome {
    // region:    --- Chainable setters

    /// Wipe the rect before painting. Use it for surfaces that float over other
    /// content; a surface that shares a background does not need it.
    pub fn with_clear(self, clear: bool) -> Self {
        Self { clear, ..self }
    }

    /// Paint the rect with `bg` before the border, so the fill reaches every
    /// cell the border leaves alone.
    pub fn with_bg(self, bg: Style) -> Self {
        Self {
            bg: Some(bg),
            ..self
        }
    }

    /// Draw the border with `border`'s glyph set.
    pub fn with_border(self, border: ContainerBorder) -> Self {
        Self { border, ..self }
    }

    /// Paint `border_style` on the border rules and corners.
    pub fn with_border_style(self, border_style: Style) -> Self {
        Self {
            border_style,
            ..self
        }
    }

    /// Put `title` on the top border row.
    pub fn with_title(self, title: impl Into<Line<'static>>) -> Self {
        Self {
            title: Some(title.into()),
            ..self
        }
    }

    /// Paint `title_style` on the title glyphs. `Block` styles titles through
    /// `title_style`, not `border_style`, so without this a title keeps whatever
    /// foreground its cells already carried.
    pub fn with_title_style(self, title_style: Style) -> Self {
        Self {
            title_style: Some(title_style),
            ..self
        }
    }

    // endregion: --- Chainable setters

    // region:    --- Draw

    /// Draw the container and return its bordered inner rect, one row and one
    /// column inside `area` — where the caller renders its content.
    pub fn draw(&self, frame: &mut Frame, area: Rect) -> Rect {
        if self.clear {
            frame.render_widget(Clear, area);
        }
        if let Some(bg) = self.bg {
            frame.render_widget(Block::default().style(bg), area);
        }
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_set(self.border.glyphs())
            .border_style(self.border_style);
        if let Some(title_style) = self.title_style {
            block = block.title_style(title_style);
        }
        if let Some(title) = self.title.clone() {
            block = block.title(title);
        }
        frame.render_widget(block, area);
        area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        })
    }

    // endregion: --- Draw
}

// endregion: --- Chrome
