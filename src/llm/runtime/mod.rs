//! Runtime boundary for LLM provider lifecycle.
//!
//! Responsibilities are strictly separated:
//! - [`key`] owns stable cache identity + auth fingerprinting
//! - [`cache`] owns concurrent get-or-create lifecycle
//! - [`factory`] owns one-time config -> concrete provider selection
//! - [`cached`] owns concrete provider execution dispatch
//!
//! Forbidden patterns:
//! - provider construction in `llm::call_llm`
//! - endpoint/model-family switching in `llm` orchestration layer

mod cache;
mod cached;
mod engine;
mod factory;
mod key;

pub use cache::ProviderCache;
pub use engine::LlmRuntime;
pub use key::{ProviderKey, auth_fingerprint};
