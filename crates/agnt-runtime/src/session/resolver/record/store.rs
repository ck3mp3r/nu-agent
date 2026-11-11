//! The store's two faces the record needs: the fact tables a session settled,
//! and the transcript read the rows answer.

// region:    --- Modules

use std::sync::Arc;

use nu_protocol::LabeledError;

use crate::session::RigStoreConfig;
use crate::session::settle::{FactStore, SessionFacts};
use agnt_storage::{RigAdapter, Storage, TranscriptRecord};

// endregion: --- Modules

// region:    --- Functions

/// The facts of `session_id`, or none when the database cannot be read.
///
/// A reload that cannot read the facts still hydrates its transcript: every
/// row then reads its stored verdict and no entry carries a turn.
pub(super) async fn read_facts(
    rig: &RigStoreConfig,
    session_id: &str,
) -> Result<SessionFacts, LabeledError> {
    let facts = match FactStore::open(rig).await {
        Ok(facts) => facts,
        Err(error) => {
            log::warn!("failed to open the session's fact tables: {error}");
            return Ok(SessionFacts::default());
        }
    };
    match facts.read(session_id).await {
        Ok(facts) => Ok(facts),
        Err(error) => {
            log::warn!("failed to read the session's facts: {error}");
            Ok(SessionFacts::default())
        }
    }
}

/// The engine of `rig`'s database.
pub(super) async fn open_store(rig: &RigStoreConfig) -> Result<Arc<Storage>, LabeledError> {
    let config = rig.clone();
    crate::session::blocking::on_pool(move || config.facts_blocking())
        .await
        .map_err(|e| LabeledError::new(format!("failed to open the fact tables: {e}")))?
        .map_err(|e| LabeledError::new(format!("failed to open the fact tables: {e}")))
}

/// The session's transcript rows, as the store reads them.
pub(super) async fn read_transcript(
    storage: Arc<Storage>,
    session_id: &str,
) -> Result<Vec<TranscriptRecord>, LabeledError> {
    let session = session_id.to_owned();
    crate::session::blocking::on_pool(move || {
        RigAdapter::new(storage, session)
            .transcript()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| LabeledError::new(format!("failed to read the transcript: {e}")))?
    .map_err(|e| LabeledError::new(format!("failed to read the transcript: {e}")))
}

// endregion: --- Functions
