//! ACP (Agent Client Protocol) dispatch for the CLI.

use crate::error::{Error, Result};

// region:    --- Functions

/// Serve the ACP protocol over stdio until the client disconnects.
///
/// `agent` selects the agent whose permissions overlay applies to every
/// session; `None` falls back to the configured default agent.
pub async fn run_acp(agent: Option<String>) -> Result<()> {
    agnt_acp::serve(agent)
        .await
        .map_err(|e| Error::Custom(format!("ACP server error: {e}")))
}

// endregion: --- Functions
