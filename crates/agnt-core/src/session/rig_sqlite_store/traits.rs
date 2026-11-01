// region:    --- Modules

use std::io;

use rig_memory::{Files, LogWriteError, Store, check_name};

use super::actor::{Job, RigSqliteStore};

// endregion: --- Modules

// region:    --- Files

impl Files for RigSqliteStore {
    fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        check_name(name)?;
        let job = Job::WriteFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
            bytes: bytes.to_vec(),
        };
        self.submit_unit(job).map_err(io::Error::other)
    }

    fn append_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        check_name(name)?;
        let job = Job::AppendFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
            bytes: bytes.to_vec(),
        };
        self.submit_unit(job).map_err(io::Error::other)
    }

    fn read_file(&self, name: &str) -> io::Result<Vec<u8>> {
        check_name(name)?;
        let job = Job::ReadFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
        };
        self.submit_row(job)
            .map_err(io::Error::other)?
            .ok_or_else(not_found)
    }

    fn remove_file(&self, name: &str) -> io::Result<()> {
        check_name(name)?;
        let job = Job::RemoveFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
        };
        self.submit_unit(job).map_err(io::Error::other)
    }

    fn files(&self) -> io::Result<Vec<String>> {
        let job = Job::Files {
            session: self.session_id.clone(),
        };
        self.submit_names(job).map_err(io::Error::other)
    }
}

// endregion: --- Files

// region:    --- Store

impl Store for RigSqliteStore {
    fn logs(&self) -> io::Result<Vec<String>> {
        let job = Job::Logs {
            session: self.session_id.clone(),
        };
        self.submit_names(job).map_err(io::Error::other)
    }

    fn read_log(&self, name: &str) -> io::Result<Vec<u8>> {
        check_leaf(name)?;
        let job = Job::ReadLog {
            session: self.session_id.clone(),
            name: name.to_owned(),
        };
        self.submit_row(job)
            .map_err(io::Error::other)?
            .ok_or_else(not_found)
    }

    fn append_log(&self, name: &str, len: u64, bytes: &[u8]) -> Result<(), LogWriteError> {
        check_leaf(name)?;
        let Ok(len) = i64::try_from(len) else {
            return Err(LogWriteError::Changed);
        };
        let job = Job::AppendLog {
            session: self.session_id.clone(),
            name: name.to_owned(),
            len,
            bytes: bytes.to_vec(),
        };
        let written = self
            .submit_written(job)
            .map_err(|error| LogWriteError::Io(io::Error::other(error)))?;
        if written {
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
        let job = Job::ReplaceLog {
            session: self.session_id.clone(),
            name: name.to_owned(),
            len,
            bytes: bytes.to_vec(),
        };
        let written = self
            .submit_written(job)
            .map_err(|error| LogWriteError::Io(io::Error::other(error)))?;
        if written {
            Ok(())
        } else {
            Err(LogWriteError::Changed)
        }
    }

    fn destroy(&self) -> io::Result<()> {
        let job = Job::Destroy {
            session: self.session_id.clone(),
        };
        self.submit_unit(job).map_err(io::Error::other)
    }
}

// endregion: --- Store

// region:    --- Support

/// Refuses `name` as a log name: a [`check_name`] in one part.
fn check_leaf(name: &str) -> io::Result<()> {
    check_name(name)?;
    if name.contains('/') {
        let why = format!("{name:?} is no log name");
        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    }
    Ok(())
}

fn not_found() -> io::Error {
    io::ErrorKind::NotFound.into()
}

// endregion: --- Support
