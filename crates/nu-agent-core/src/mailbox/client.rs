use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::protocol::{ClientFrame, ServerFrame};

/// Error type for broker client operations
#[derive(Debug, thiserror::Error)]
pub enum BrokerClientError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Authentication rejected: {0}")]
    AuthRejected(String),
    #[error("Unexpected frame received")]
    UnexpectedFrame,
    #[error("Disconnected from broker")]
    Disconnected,
}

/// Broker client for agent communication
pub struct BrokerClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
    pub name: String,
}

impl std::fmt::Debug for BrokerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerClient")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl BrokerClient {
    /// Connect to broker and authenticate
    pub async fn connect(socket_path: &Path, token: &str) -> Result<Self, BrokerClientError> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        // Send auth frame
        let auth = serde_json::to_string(&ClientFrame::Auth {
            token: token.to_string(),
        })? + "\n";
        write_half.write_all(auth.as_bytes()).await?;

        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let response: ServerFrame = serde_json::from_str(line.trim())?;

        match response {
            ServerFrame::AuthOk { name } => Ok(Self {
                reader,
                writer: write_half,
                name,
            }),
            ServerFrame::AuthRejected { reason } => Err(BrokerClientError::AuthRejected(reason)),
            _ => Err(BrokerClientError::UnexpectedFrame),
        }
    }

    /// Split into sender and receiver for separate usage
    pub fn split(self) -> (BrokerSender, BrokerReceiver) {
        (
            BrokerSender {
                writer: self.writer,
            },
            BrokerReceiver {
                reader: self.reader,
            },
        )
    }

    /// Send a message to another agent (test-only convenience)
    #[cfg(test)]
    pub async fn send(
        &mut self,
        to: &str,
        message: &str,
        kind: &str,
    ) -> Result<(), BrokerClientError> {
        let frame = serde_json::to_string(&ClientFrame::Message {
            to: to.to_string(),
            message: message.to_string(),
            kind: kind.to_string(),
        })? + "\n";
        self.writer.write_all(frame.as_bytes()).await?;
        Ok(())
    }

    /// Receive a frame from the broker (test-only convenience)
    #[cfg(test)]
    pub async fn recv(&mut self) -> Result<ServerFrame, BrokerClientError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(BrokerClientError::Disconnected);
        }
        Ok(serde_json::from_str(line.trim())?)
    }
}

/// Send-only half of broker client
pub struct BrokerSender {
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl BrokerSender {
    /// Send a message asynchronously
    pub async fn send(
        &mut self,
        to: &str,
        message: &str,
        kind: &str,
    ) -> Result<(), BrokerClientError> {
        let frame = serde_json::to_string(&ClientFrame::Message {
            to: to.to_string(),
            message: message.to_string(),
            kind: kind.to_string(),
        })? + "\n";
        self.writer.write_all(frame.as_bytes()).await?;
        Ok(())
    }

    #[cfg(test)]
    pub fn new_for_test(writer: tokio::net::unix::OwnedWriteHalf) -> Self {
        Self { writer }
    }
}

/// Receive-only half of broker client
pub struct BrokerReceiver {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
}

impl BrokerReceiver {
    /// Receive a frame from the broker (async)
    pub async fn recv(&mut self) -> Result<ServerFrame, BrokerClientError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(BrokerClientError::Disconnected);
        }
        Ok(serde_json::from_str(line.trim())?)
    }
}
