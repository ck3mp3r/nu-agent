//! ACP (Agent Client Protocol) dispatch for the CLI.

use crate::error::{Error, Result};

// region:    --- Functions

/// Serve the ACP protocol over stdio until the client disconnects.
pub async fn run_acp() -> Result<()> {
    agnt_acp::serve()
        .await
        .map_err(|e| Error::Custom(format!("ACP server error: {e}")))
}

// endregion: --- Functions
