use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::protocol::{ClientFrame, ServerFrame};
use super::registry::AgentRegistry;

pub(crate) struct Broker {
    socket_dir: PathBuf,
    socket_path: PathBuf,
    shutdown: CancellationToken,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BrokerError {
    #[error("XDG_RUNTIME_DIR environment variable not set")]
    XdgRuntimeDirNotSet,
    #[error("Failed to create socket: {0}")]
    SocketCreationFailed(#[from] std::io::Error),
    #[error("Failed to create directory: {0}")]
    DirectoryCreationFailed(std::io::Error),
}

impl Broker {
    pub(crate) fn start(registry: Arc<RwLock<AgentRegistry>>) -> Result<Self, BrokerError> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .or_else(|_| std::env::var("TMPDIR"))
            .or_else(|_| Ok::<String, std::env::VarError>("/tmp".to_string()))
            .map_err(|_| BrokerError::XdgRuntimeDirNotSet)?;

        // Generate random directory name using hash of timestamp + process ID + thread ID
        // Keep it short to avoid SUN_LEN limit on Unix domain sockets (typically 108 bytes)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .hash(&mut hasher);
        std::process::id().hash(&mut hasher);
        format!("{:?}", std::thread::current().id()).hash(&mut hasher);
        let random_suffix = format!("{:x}", hasher.finish());

        let socket_dir = PathBuf::from(runtime_dir).join(format!("nu-agent-{}", random_suffix));

        // Create directory with 0700 permissions
        std::fs::create_dir_all(&socket_dir).map_err(BrokerError::DirectoryCreationFailed)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o700);
            std::fs::set_permissions(&socket_dir, perms)
                .map_err(BrokerError::DirectoryCreationFailed)?;
        }

        let socket_path = socket_dir.join("broker.sock");
        let listener = UnixListener::bind(&socket_path)?;

        let shutdown = CancellationToken::new();
        let accept_shutdown = shutdown.clone();
        let accept_registry = registry.clone();
        let accept_socket_path = socket_path.clone();

        tokio::spawn(async move {
            log::debug!("Broker accept loop started on {:?}", accept_socket_path);
            loop {
                tokio::select! {
                    _ = accept_shutdown.cancelled() => {
                        log::debug!("Broker accept loop shutting down");
                        break;
                    }
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _addr)) => {
                                let conn_shutdown = accept_shutdown.child_token();
                                let conn_registry = accept_registry.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, conn_registry, conn_shutdown).await {
                                        log::debug!("Connection handler error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                log::debug!("Accept error: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            socket_dir,
            socket_path,
            shutdown,
        })
    }

    async fn handle_connection(
        stream: tokio::net::UnixStream,
        registry: Arc<RwLock<AgentRegistry>>,
        shutdown: CancellationToken,
    ) -> Result<(), String> {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line_buffer = String::new();

        // Read and parse auth frame
        line_buffer.clear();
        tokio::select! {
            _ = shutdown.cancelled() => {
                return Ok(());
            }
            result = reader.read_line(&mut line_buffer) => {
                result.map_err(|e| format!("Failed to read auth frame: {}", e))?;
            }
        }

        let auth_frame: ClientFrame = serde_json::from_str(line_buffer.trim())
            .map_err(|e| format!("Failed to parse auth frame: {}", e))?;

        let agent_name = match auth_frame {
            ClientFrame::Auth { token } => {
                // Authenticate
                let mut reg = registry.write().await;
                match reg.authenticate(&token) {
                    Some(name) => {
                        // Create message channel
                        let (tx, mut rx) = tokio::sync::mpsc::channel::<ServerFrame>(100);
                        reg.add_connected(name.clone(), tx);
                        drop(reg); // Release lock before sending

                        // Send auth success
                        let auth_ok = ServerFrame::AuthOk { name: name.clone() };
                        let frame_line = serde_json::to_string(&auth_ok)
                            .map_err(|e| format!("Failed to serialize AuthOk: {}", e))?;
                        write_half
                            .write_all(format!("{}\n", frame_line).as_bytes())
                            .await
                            .map_err(|e| format!("Failed to write AuthOk: {}", e))?;

                        // Spawn writer task
                        let writer_shutdown = shutdown.child_token();
                        let mut write_half_clone = write_half;
                        tokio::spawn(async move {
                            loop {
                                tokio::select! {
                                    _ = writer_shutdown.cancelled() => {
                                        break;
                                    }
                                    frame = rx.recv() => {
                                        match frame {
                                            Some(frame) => {
                                                if let Ok(frame_line) = serde_json::to_string(&frame) {
                                                    let _ = write_half_clone
                                                        .write_all(format!("{}\n", frame_line).as_bytes())
                                                        .await;
                                                }
                                            }
                                            None => break,
                                        }
                                    }
                                }
                            }
                        });

                        name
                    }
                    None => {
                        // Send auth rejection
                        let auth_reject = ServerFrame::AuthRejected {
                            reason: "Invalid token".to_string(),
                        };
                        let frame_line = serde_json::to_string(&auth_reject)
                            .map_err(|e| format!("Failed to serialize AuthRejected: {}", e))?;
                        write_half
                            .write_all(format!("{}\n", frame_line).as_bytes())
                            .await
                            .map_err(|e| format!("Failed to write AuthRejected: {}", e))?;
                        return Err("Authentication failed".to_string());
                    }
                }
            }
            _ => {
                return Err("Expected Auth frame".to_string());
            }
        };

        // Message routing loop
        loop {
            line_buffer.clear();
            let bytes_read = tokio::select! {
                _ = shutdown.cancelled() => {
                    log::debug!("Connection shutdown signal received");
                    break;
                }
                result = reader.read_line(&mut line_buffer) => {
                    result.map_err(|e| format!("Failed to read message: {}", e))?
                }
            };

            if bytes_read == 0 {
                break;
            }

            let frame: ClientFrame = match serde_json::from_str(line_buffer.trim()) {
                Ok(f) => f,
                Err(e) => {
                    log::debug!("Failed to parse frame: {}", e);
                    continue;
                }
            };

            match frame {
                ClientFrame::Message { to, message, kind } => {
                    let reg = registry.read().await;
                    let server_frame = ServerFrame::Message {
                        from: agent_name.clone(),
                        message,
                        kind,
                    };
                    if let Err(e) = reg.route_message(&to, server_frame) {
                        log::debug!("Failed to route message: {}", e);
                    }
                }
                ClientFrame::Auth { .. } => {
                    log::debug!("Unexpected Auth frame after authentication");
                }
            }
        }

        // Clean up on disconnect
        let mut reg = registry.write().await;
        reg.remove_connected(&agent_name);
        log::debug!("Agent '{}' disconnected", agent_name);

        Ok(())
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.shutdown.cancel();
        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_dir(&self.socket_dir);
    }
}
