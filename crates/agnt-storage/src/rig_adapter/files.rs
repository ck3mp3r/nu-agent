//! rig's [`Files`] contract: the session's named files, as blob rows.

// region:    --- Modules

use std::io;

use rig_memory::{Files, check_name};

use super::work::{AppendFile, FileNames, ReadFile, RemoveFile, WriteFile};
use super::{RigAdapter, not_found, sql_error};

// endregion: --- Modules

// region:    --- Files

impl Files for RigAdapter {
    fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        check_name(name)?;
        let work = WriteFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
            bytes: bytes.to_vec(),
        };
        self.call(work)?.map_err(sql_error)
    }

    fn append_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        check_name(name)?;
        let work = AppendFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
            bytes: bytes.to_vec(),
        };
        self.call(work)?.map_err(sql_error)
    }

    fn read_file(&self, name: &str) -> io::Result<Vec<u8>> {
        check_name(name)?;
        let work = ReadFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
        };
        self.call(work)?.map_err(sql_error)?.ok_or_else(not_found)
    }

    fn remove_file(&self, name: &str) -> io::Result<()> {
        check_name(name)?;
        let work = RemoveFile {
            session: self.session_id.clone(),
            name: name.to_owned(),
        };
        self.call(work)?.map_err(sql_error)
    }

    fn files(&self) -> io::Result<Vec<String>> {
        let work = FileNames {
            session: self.session_id.clone(),
        };
        self.call(work)?.map_err(sql_error)
    }
}

// endregion: --- Files
