mod broker;
mod client;
mod protocol;
mod registry;

#[allow(unused_imports)]
pub use broker::{Broker, BrokerError};
#[allow(unused_imports)]
pub use client::{BrokerClient, BrokerClientError, BrokerReceiver, BrokerSender};
#[allow(unused_imports)]
pub use protocol::{ClientFrame, IncomingMessage, ServerFrame};
#[allow(unused_imports)]
pub use registry::AgentRegistry;

#[cfg(test)]
mod broker_test;
#[cfg(test)]
mod client_test;
#[cfg(test)]
mod protocol_test;
#[cfg(test)]
mod registry_test;
