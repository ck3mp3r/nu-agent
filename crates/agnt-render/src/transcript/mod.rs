pub mod entry;
pub mod highlight;
pub mod ir;
pub mod markdown;
pub mod renderer;
pub mod syntax_packdump;

#[cfg(test)]
#[path = "../../test/transcript/highlight.rs"]
mod highlight_test;
#[cfg(test)]
#[path = "../../test/transcript/ir.rs"]
mod ir_test;
#[cfg(all(test, feature = "integration"))]
#[path = "../../test/transcript/packdump_integration.rs"]
mod packdump_integration;
#[cfg(test)]
#[path = "../../test/transcript/renderer.rs"]
mod renderer_test;
#[cfg(test)]
#[path = "../../test/transcript/syntax_packdump.rs"]
mod syntax_packdump_test;
