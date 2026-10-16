use ratatui::style::Color;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) const MOCHA_RED: Color = Color::Rgb(243, 139, 168);
pub(super) const MOCHA_YELLOW: Color = Color::Rgb(249, 226, 175);
pub(super) const MOCHA_GREEN: Color = Color::Rgb(166, 227, 161);
pub(super) const MOCHA_BLUE: Color = Color::Rgb(137, 180, 250);
pub(super) const MOCHA_SAPPHIRE: Color = Color::Rgb(116, 199, 236);
pub(super) const MOCHA_LAVENDER: Color = Color::Rgb(180, 190, 254);
pub(super) const MOCHA_MAUVE: Color = Color::Rgb(203, 166, 247);
pub(super) const MOCHA_PEACH: Color = Color::Rgb(250, 179, 135);
pub(super) const MOCHA_OVERLAY0: Color = Color::Rgb(108, 112, 134);
pub(super) const MOCHA_OVERLAY1: Color = Color::Rgb(127, 132, 156);
pub(super) const MOCHA_OVERLAY2: Color = Color::Rgb(147, 153, 178);
pub(super) const MOCHA_SKY: Color = Color::Rgb(137, 220, 235);
pub(super) const MOCHA_TEXT: Color = Color::Rgb(205, 214, 244);
