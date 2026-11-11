//! The runtime hook chain.
//!
//! Each concern (cancellation, sub-turn cap, doom loop, circuit breaker, history
//! snapshot) lives in its own module and has its own unit tests. `HookChain` owns
//! them as named fields and delegates to them in an explicit, readable order inside
//! the `AgentHook` impl.
//!
//! ## Adding a new concern
//!
//! 1. Create `new_concern.rs` plus its test under `test/hook/`.
//! 2. Add a named field on `HookChain`.
//! 3. Call the concern's method inside whichever event method needs it.
//! 4. Nothing else changes.

// region:    --- Modules

pub mod adapter;
pub mod agent_hook;
pub mod chain;
pub mod circuit_breaker_guard;
pub mod doom_loop;
pub mod history_snapshot;
pub mod output_repetition;
pub mod permission_resolver;
pub mod subturn_cap;
pub mod turn_tools;
pub mod turn_usage;
pub mod unpriced_warnings;

pub use adapter::{ClosureToolAdapter, adapt_closures};
pub use agent_hook::{DoomLoopState, HookState, StopLedger, StoreLatch};
pub use chain::HookChain;
pub use permission_resolver::{
    AsyncPermissionResolver, InteractivePermissionResolver, PermissionDecision,
    PolicyPermissionResolver,
};
pub use turn_tools::{TurnToolLog, TurnToolRecord};
pub use turn_usage::{TurnUsage, TurnUsageTotals};

// endregion: --- Modules
