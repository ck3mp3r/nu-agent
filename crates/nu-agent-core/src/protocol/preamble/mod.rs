pub mod classify;
pub mod defaults;
pub mod resolve;

pub use classify::classify_model_family;
pub use defaults::PreambleDefaults;
pub use resolve::{UserPreambleInput, resolve_preamble};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelFamily {
    Gpt5x,
    Gpt4x,
    Anthropic,
    AnthropicSonnet,
    Unknown,
}

#[cfg(test)]
mod test;
