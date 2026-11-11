//! The control concern: commands addressed to the loop owner.
//!
//! Delivery contract: never dropped. A subscriber attached before a publish
//! observes every later command in publication order, with no capacity
//! ceiling — a command that is lost is a command that never ran. Zero
//! retention: a command published with no subscriber attached is returned to
//! the publisher, because control is consumed forward and never replayed.

use crate::bus::events::ControlCommand;
use crate::bus::ring::{PublishError, Strict, StrictSubscription};

/// Never-dropped publication of [`ControlCommand`]s.
#[derive(Clone)]
pub struct Control {
    strict: Strict<ControlCommand>,
}

impl Control {
    /// The control concern. Control has no capacity: the queue behind each
    /// subscriber grows with the commands it has not read yet.
    pub fn new() -> Self {
        Self {
            strict: Strict::new(),
        }
    }

    /// Publish one command to every live subscriber.
    pub fn publish(&self, command: ControlCommand) -> Result<usize, PublishError> {
        self.strict.publish(command)
    }

    /// Subscribe a consumer. From here on it observes every published command.
    pub fn subscribe(&self) -> StrictSubscription<ControlCommand> {
        self.strict.subscribe()
    }

    /// The number of live subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.strict.subscriber_count()
    }
}

impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}
