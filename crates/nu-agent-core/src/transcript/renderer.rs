use super::ir::RenderBlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    InProgress,
    Done,
    Failed,
    Queued,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub width: usize,
    pub cursor: bool,
    pub selected: bool,
    pub status: Option<ItemStatus>,
    pub now_millis: u128,
}

pub trait BlockRenderer {
    type Output;
    fn render(&self, block: &RenderBlock, ctx: &RenderContext) -> Self::Output;
}
