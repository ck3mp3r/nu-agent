use std::collections::VecDeque;

use nu_agent_core::protocol::event::UiEvent;

use crate::interaction::reducer::{ReducerInput, UserAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportItem {
    User(UserAction),
    Event(Box<UiEvent>),
}

impl From<TransportItem> for ReducerInput {
    fn from(value: TransportItem) -> Self {
        match value {
            TransportItem::User(action) => ReducerInput::User(action),
            TransportItem::Event(event) => ReducerInput::Event(event),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    User,
    Event,
}

/// Transport boundary between event producers (terminal input and UI events)
/// and the reducer-driving consumer loop.
///
/// Queue policy: unbounded queues per source.
/// Rationale: this module is a boundary abstraction only; it should not drop
/// events/actions before runtime backpressure strategy is implemented.
#[derive(Debug)]
pub struct TuiTransport {
    user_actions: VecDeque<UserAction>,
    ui_events: VecDeque<UiEvent>,
    next_when_both: Source,
}

impl TuiTransport {
    pub fn enqueue_user_action(&mut self, action: UserAction) {
        self.user_actions.push_back(action);
    }

    pub fn enqueue_ui_event(&mut self, event: UiEvent) {
        self.ui_events.push_back(event);
    }

    pub fn poll_next(&mut self) -> Option<TransportItem> {
        match (self.user_actions.is_empty(), self.ui_events.is_empty()) {
            (true, true) => None,
            (false, true) => self.user_actions.pop_front().map(TransportItem::User),
            (true, false) => self
                .ui_events
                .pop_front()
                .map(|e| TransportItem::Event(Box::new(e))),
            (false, false) => {
                let from_user = self.next_when_both == Source::User;
                self.next_when_both = if from_user {
                    Source::Event
                } else {
                    Source::User
                };

                if from_user {
                    self.user_actions.pop_front().map(TransportItem::User)
                } else {
                    self.ui_events
                        .pop_front()
                        .map(|e| TransportItem::Event(Box::new(e)))
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.user_actions.is_empty() && self.ui_events.is_empty()
    }
}

impl Default for TuiTransport {
    fn default() -> Self {
        Self {
            user_actions: VecDeque::new(),
            ui_events: VecDeque::new(),
            next_when_both: Source::User,
        }
    }
}
