pub mod highlight;
pub mod ir;
pub mod items;
pub mod markdown;
pub mod renderer;

#[cfg(test)]
#[path = "../../test/transcript/highlight.rs"]
mod highlight_test;
#[cfg(test)]
#[path = "../../test/transcript/ir.rs"]
mod ir_test;
#[cfg(test)]
#[path = "../../test/transcript/items.rs"]
mod items_test;
#[cfg(test)]
#[path = "../../test/transcript/renderer.rs"]
mod renderer_test;
