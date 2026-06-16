pub mod builtin;
pub mod closure;

pub use builtin::{BuiltinToolAdapter, adapt_builtins};
pub use closure::{ClosureToolAdapter, adapt_closures};
