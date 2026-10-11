use nu_protocol::{Spanned, engine::Closure};

use super::conversion::ClosureParameter;

/// A closure with pre-extracted parameter metadata.
/// Parameters are resolved eagerly using the original EngineInterface
/// before it is cloned, avoiding the dead-context problem.
#[derive(Debug, Clone)]
pub struct ResolvedClosure {
    pub closure: Spanned<Closure>,
    pub params: Vec<ClosureParameter>,
}
