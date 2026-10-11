//! Minimal engine implementation for ACP mode.

// region:    --- Types

/// Engine stub for ACP mode.
///
/// An ACP session has no Nushell engine: there is no span source and no closure
/// evaluation. The stub supplies the session working directory — the only
/// engine capability the runtime needs — and reports every other capability as
/// unavailable.
pub use agnt_core::tools::closure::CwdEngine as AcpEngineStub;

// endregion: --- Types
