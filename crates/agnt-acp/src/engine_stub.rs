//! Minimal engine implementation for ACP mode.

// region:    --- Types

/// Engine stub for ACP mode.
///
/// An ACP session has no Nushell engine: there is no span source and no closure
/// evaluation. The stub reports every capability as unavailable.
pub use agnt_runtime::tools::closure::NoopEvaluator as AcpEngineStub;

// endregion: --- Types
