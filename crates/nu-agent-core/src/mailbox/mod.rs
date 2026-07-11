mod broker;
mod client;
mod protocol;

pub(crate) use broker::MailboxHandle;
pub(crate) use broker::socket_dir_for_path;
pub use broker::{AgentMailbox, MailboxError};
pub use client::{SendError, send_to};
pub use protocol::{IncomingMessage, MessageFrame};

#[cfg(test)]
mod test;
