use agnt_kernel::bus::{CancelTx, ControlTx, FactsTx, LifecycleTx, NarrationTx, Stream};

/// The runtime bus: the six D4 concerns, one hand each.
///
/// Each concern carries its own payload type, so the compiler enforces that,
/// for example, a `SessionRecord` can never be sent on the cancel channel.
///
/// The concerns are the kernel's: the ordered `stream` (session records), the
/// strict `cancel`, the never-dropped `control` (commands to the loop owner),
/// the ordered and complete `lifecycle` (the turn spine plus the session, task
/// and compaction rows), the oldest-droppable `narration` (text for a human),
/// and the latest-wins `facts` (kernel state snapshots).
#[derive(Clone)]
pub struct Bus {
    stream: Stream,
    cancel: CancelTx,
    control: ControlTx,
    lifecycle: LifecycleTx,
    narration: NarrationTx,
    facts: FactsTx,
}

impl Bus {
    /// The ordered session-record stream — the one log every surface folds.
    pub fn stream(&self) -> &Stream {
        &self.stream
    }
    /// The strict cancellation concern.
    pub fn cancel(&self) -> &CancelTx {
        &self.cancel
    }
    /// The never-dropped commands addressed to the loop owner.
    pub fn control(&self) -> &ControlTx {
        &self.control
    }
    /// The ordered, complete turn spine and its session, task and compaction
    /// rows.
    pub fn lifecycle(&self) -> &LifecycleTx {
        &self.lifecycle
    }
    /// The ordered text stream for a human reader.
    pub fn narration(&self) -> &NarrationTx {
        &self.narration
    }
    /// The latest-wins kernel state snapshots.
    pub fn facts(&self) -> &FactsTx {
        &self.facts
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            stream: Stream::new(),
            cancel: CancelTx::new("cancel", 64),
            control: ControlTx::new(),
            lifecycle: LifecycleTx::new(),
            narration: NarrationTx::new(),
            facts: FactsTx::new(),
        }
    }
}
