use std::collections::VecDeque;

use agnt_core::protocol::events::SystemEvent;

/// Where a transport item came from: the terminal input boundary, or any event
/// producer on the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Input,
    Event,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransportItem {
    pub source: Source,
    pub event: SystemEvent,
}

/// Transport boundary between event producers (terminal input and the event
/// bus) and the reducer-driving consumer loop.
///
/// Queue policy: one unbounded queue per source, alternating when both carry
/// work. Rationale: this module is a boundary abstraction only; it must not drop
/// events before a runtime backpressure strategy is implemented. The one drop
/// rule — streaming chunk coalescing — belongs to the consumer, which owns the
/// drain cycle and states it there.
#[derive(Debug)]
pub struct TuiTransport {
    input: VecDeque<SystemEvent>,
    events: VecDeque<SystemEvent>,
    next_when_both: Source,
}

impl TuiTransport {
    pub fn enqueue_input(&mut self, event: SystemEvent) {
        self.input.push_back(event);
    }

    pub fn enqueue_event(&mut self, event: SystemEvent) {
        self.events.push_back(event);
    }

    pub fn poll_next(&mut self) -> Option<TransportItem> {
        let (source, event) = match (self.input.is_empty(), self.events.is_empty()) {
            (true, true) => return None,
            (false, true) => (Source::Input, self.input.pop_front()?),
            (true, false) => (Source::Event, self.events.pop_front()?),
            (false, false) => {
                let from_input = self.next_when_both == Source::Input;
                self.next_when_both = if from_input {
                    Source::Event
                } else {
                    Source::Input
                };
                if from_input {
                    (Source::Input, self.input.pop_front()?)
                } else {
                    (Source::Event, self.events.pop_front()?)
                }
            }
        };
        Some(TransportItem { source, event })
    }

    pub fn is_empty(&self) -> bool {
        self.input.is_empty() && self.events.is_empty()
    }
}

impl Default for TuiTransport {
    fn default() -> Self {
        Self {
            input: VecDeque::new(),
            events: VecDeque::new(),
            next_when_both: Source::Input,
        }
    }
}
