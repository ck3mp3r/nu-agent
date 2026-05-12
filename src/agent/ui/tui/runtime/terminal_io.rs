use std::{fs::File, io::Read, os::fd::AsRawFd, time::Duration};

use crate::agent::ui::tui::interaction::input::{TerminalEvent, TerminalKey};

use super::terminal_events::TerminalEventSource;

#[derive(Debug)]
pub struct TtyTerminalEvents {
    reader: File,
}

impl TtyTerminalEvents {
    pub fn new(tty_reader: File, _poll_timeout: Duration) -> Result<Self, String> {
        set_nonblocking(&tty_reader)?;
        Ok(Self { reader: tty_reader })
    }
}

impl TerminalEventSource for TtyTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        let mut buf = [0u8; 1];
        match self.reader.read(&mut buf) {
            Ok(0) => Err("/dev/tty returned EOF".to_string()),
            Ok(_) => Ok(map_tty_byte(buf[0])),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => Ok(None),
            Err(err) if err.raw_os_error() == Some(5) => Ok(None),
            Err(err) => Err(format!("/dev/tty read failed: {err}")),
        }
    }
}

fn set_nonblocking(file: &File) -> Result<(), String> {
    let fd = file.as_raw_fd();
    // SAFETY: fcntl is called with valid fd and standard F_GETFL/F_SETFL commands.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(format!(
            "failed to query /dev/tty file status flags: {}",
            std::io::Error::last_os_error()
        ));
    }

    // SAFETY: same as above; we set O_NONBLOCK while preserving existing flags.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if rc < 0 {
        return Err(format!(
            "failed to enable non-blocking mode for /dev/tty: {}",
            std::io::Error::last_os_error()
        ));
    }

    Ok(())
}

fn map_tty_byte(byte: u8) -> Option<TerminalEvent> {
    let key = match byte {
        3 => TerminalKey::CtrlC,
        b'\r' | b'\n' => TerminalKey::Enter,
        8 | 127 => TerminalKey::Backspace,
        27 => TerminalKey::Esc,
        value if value.is_ascii_graphic() || value == b' ' => TerminalKey::Char(value as char),
        _ => return None,
    };

    Some(TerminalEvent::Key(key))
}

pub fn open_tty_reader() -> Result<File, String> {
    std::fs::OpenOptions::new()
        .read(true)
        .open("/dev/tty")
        .map_err(|err| format!("failed to open /dev/tty for reading: {err}"))
}
