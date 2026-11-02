//! rig's [`Store`] contract: its logs, with the compare-and-set writes that
//! are rig's durability contract.

// region:    --- Modules

use std::io;

use rig_memory::{LogWriteError, Store, check_name};

use super::work::{AppendLog, Destroy, LogNames, ReadLog, ReplaceLog};
use super::{RigAdapter, not_found, sql_error};

// endregion: --- Modules

// region:    --- Store

impl Store for RigAdapter {
    fn logs(&self) -> io::Result<Vec<String>> {
        let work = LogNames {
            session: self.session_id.clone(),
        };
        self.call(work)?.map_err(sql_error)
    }

    fn read_log(&self, name: &str) -> io::Result<Vec<u8>> {
        check_leaf(name)?;
        let work = ReadLog {
            session: self.session_id.clone(),
            name: name.to_owned(),
        };
        self.call(work)?.map_err(sql_error)?.ok_or_else(not_found)
    }

    fn append_log(&self, name: &str, len: u64, bytes: &[u8]) -> Result<(), LogWriteError> {
        check_leaf(name)?;
        let Ok(len) = i64::try_from(len) else {
            return Err(LogWriteError::Changed);
        };
        let work = AppendLog {
            session: self.session_id.clone(),
            name: name.to_owned(),
            len,
            bytes: bytes.to_vec(),
        };
        let landed = self
            .call(work)
            .and_then(|result| result.map_err(sql_error))
            .map_err(LogWriteError::Io)?;
        if landed {
            Ok(())
        } else {
            Err(LogWriteError::Changed)
        }
    }

    fn replace_log(&self, name: &str, len: u64, bytes: &[u8]) -> Result<(), LogWriteError> {
        check_leaf(name)?;
        let Ok(len) = i64::try_from(len) else {
            return Err(LogWriteError::Changed);
        };
        let work = ReplaceLog {
            session: self.session_id.clone(),
            name: name.to_owned(),
            len,
            bytes: bytes.to_vec(),
        };
        let landed = self
            .call(work)
            .and_then(|result| result.map_err(sql_error))
            .map_err(LogWriteError::Io)?;
        if landed {
            Ok(())
        } else {
            Err(LogWriteError::Changed)
        }
    }

    fn destroy(&self) -> io::Result<()> {
        let work = Destroy {
            session: self.session_id.clone(),
        };
        self.call(work)?.map_err(sql_error)
    }
}

// endregion: --- Store

// region:    --- Support

/// Refuses `name` as a log name: a [`check_name`] in one part.
///
/// rig keeps this guard private, so the store that serves its contract keeps
/// its own copy: a log name is one part, never a path.
fn check_leaf(name: &str) -> io::Result<()> {
    check_name(name)?;
    if name.contains('/') {
        let why = format!("{name:?} is no log name");
        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    }
    Ok(())
}

// endregion: --- Support
