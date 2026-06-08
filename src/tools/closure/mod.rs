use nu_protocol::{Spanned, engine::Closure};

pub mod conversion;
pub mod registry;

pub use conversion::{
    ClosureParameter, EngineInterfaceLike, closure_to_tool_definition, resolve_closure_params,
};
pub use registry::ClosureRegistry;

/// A closure with pre-extracted parameter metadata.
/// Parameters are resolved eagerly using the original EngineInterface
/// before it is cloned, avoiding the dead-context problem.
#[derive(Debug, Clone)]
pub struct ResolvedClosure {
    pub closure: Spanned<Closure>,
    pub params: Vec<ClosureParameter>,
}
