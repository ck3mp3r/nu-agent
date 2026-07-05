use super::protocol::MessageFrame;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Target agent socket not found: {0}")]
    SocketNotFound(String),
}

pub async fn send_to(
    socket_dir: &Path,
    target: &str,
    from: &str,
    message: &str,
    kind: &str,
) -> Result<(), SendError> {
    let socket_path = socket_dir.join(format!("{}.sock", target));
    if !socket_path.exists() {
        return Err(SendError::SocketNotFound(socket_path.display().to_string()));
    }
    let mut stream = UnixStream::connect(&socket_path).await?;
    let line = serde_json::to_string(&MessageFrame {
        from: from.to_string(),
        message: message.to_string(),
        kind: kind.to_string(),
    })? + "\n";
    stream.write_all(line.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}
