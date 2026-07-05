use std::path::{Path, PathBuf};
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;

use super::protocol::{IncomingMessage, MessageFrame};

pub(crate) fn socket_dir_for_path(cwd: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    // XDG_RUNTIME_DIR is preferred (systemd sets it to /run/user/<uid> which is short).
    // Fall back to /tmp — NOT std::env::temp_dir() which on macOS expands to
    // /var/folders/…/T/ (>40 chars), easily pushing socket paths over the
    // 104-byte SUN_LEN limit.
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    let hash = Sha256::digest(cwd.as_os_str().as_encoded_bytes());
    let hash_prefix = hash[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    runtime_dir.join("nu-agent").join(hash_prefix)
}

#[derive(Debug, thiserror::Error)]
pub enum MailboxError {
    #[error("Failed to create socket directory: {0}")]
    DirectoryCreationFailed(std::io::Error),
    #[error("Failed to bind socket: {0}")]
    SocketBindFailed(std::io::Error),
}

/// Prepared but not yet started mailbox. Created outside the Tokio runtime
/// (filesystem work only). Call `start()` from within the runtime.
pub(crate) struct MailboxHandle {
    socket_path: PathBuf,
    std_listener: std::os::unix::net::UnixListener,
}

impl MailboxHandle {
    /// Bind the socket synchronously. Safe to call outside a Tokio runtime.
    pub(crate) fn prepare(socket_dir: &Path, name: &str) -> Result<Self, MailboxError> {
        std::fs::create_dir_all(socket_dir).map_err(MailboxError::DirectoryCreationFailed)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(socket_dir, std::fs::Permissions::from_mode(0o700))
                .map_err(MailboxError::DirectoryCreationFailed)?;
        }
        let socket_path = socket_dir.join(format!("{}.sock", name));
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }
        let std_listener = std::os::unix::net::UnixListener::bind(&socket_path)
            .map_err(MailboxError::SocketBindFailed)?;
        std_listener
            .set_nonblocking(true)
            .map_err(MailboxError::SocketBindFailed)?;
        Ok(Self {
            socket_path,
            std_listener,
        })
    }

    /// Returns the socket path this handle is bound to.
    pub(crate) fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Start the async accept loop. MUST be called from within a Tokio runtime.
    pub(crate) fn start(
        self,
    ) -> Result<(AgentMailbox, std::sync::mpsc::Receiver<IncomingMessage>), MailboxError> {
        // Use ManuallyDrop to move fields out of self without triggering our
        // Drop impl (which would remove the socket file prematurely).
        let mut this = std::mem::ManuallyDrop::new(self);
        // SAFETY: we own `this` exclusively and will not access it again.
        let socket_path = unsafe { std::ptr::read(&raw const this.socket_path) };
        let std_listener = unsafe { std::ptr::read(&raw const this.std_listener) };
        // Suppress the unused-mut warning — ManuallyDrop requires &mut for ptr::read.
        let _ = &mut this;

        let listener =
            UnixListener::from_std(std_listener).map_err(MailboxError::SocketBindFailed)?;
        let (std_tx, std_rx) = std::sync::mpsc::channel::<IncomingMessage>();
        let shutdown = CancellationToken::new();
        let accept_shutdown = shutdown.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = accept_shutdown.cancelled() => break,
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let tx = std_tx.clone();
                                tokio::spawn(async move {
                                    let mut lines =
                                        tokio::io::BufReader::new(stream).lines();
                                    while let Ok(Some(line)) = lines.next_line().await {
                                        if let Ok(frame) =
                                            serde_json::from_str::<MessageFrame>(&line)
                                        {
                                            let _ = tx.send(IncomingMessage {
                                                from: frame.from,
                                                message: frame.message,
                                                kind: frame.kind,
                                            });
                                        }
                                    }
                                });
                            }
                            Err(e) => log::debug!("AgentMailbox accept error: {e}"),
                        }
                    }
                }
            }
        });
        Ok((
            AgentMailbox {
                socket_path,
                shutdown,
            },
            std_rx,
        ))
    }
}

impl Drop for MailboxHandle {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

pub struct AgentMailbox {
    socket_path: PathBuf,
    shutdown: CancellationToken,
}

impl AgentMailbox {
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for AgentMailbox {
    fn drop(&mut self) {
        self.shutdown.cancel();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
