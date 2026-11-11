#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelFamily {
    Gpt5x,
    Gpt4x,
    Anthropic,
    AnthropicSonnet,
    Unknown,
}
