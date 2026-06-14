use crate::agent::protocol::event::UiEvent;
use crate::agent::ui::tui::{
    interaction::reducer::UserAction,
    platform::transport::{TransportItem, TuiTransport},
};

#[derive(Clone)]
enum Enqueue {
    User(UserAction),
    Event(Box<UiEvent>),
}

fn completed(tool_calls: usize) -> UiEvent {
    UiEvent::Completed { tool_calls }
}

fn warning(msg: &str) -> UiEvent {
    UiEvent::Warning {
        message: msg.to_string(),
    }
}

fn drain(transport: &mut TuiTransport) -> Vec<TransportItem> {
    let mut out = Vec::new();
    while let Some(item) = transport.poll_next() {
        out.push(item);
    }
    out
}

#[test]
fn poll_next_returns_none_when_empty_non_blocking() {
    let mut transport = TuiTransport::new();

    assert!(transport.poll_next().is_none());
    assert!(transport.is_empty());
}

#[test]
fn table_driven_fifo_and_deterministic_merge_policy_round_robin_user_first() {
    struct Case {
        name: &'static str,
        enqueue: Vec<Enqueue>,
        expected: Vec<TransportItem>,
    }

    let cases = vec![
        Case {
            name: "fifo for user source only",
            enqueue: vec![
                Enqueue::User(UserAction::InsertChar('a')),
                Enqueue::User(UserAction::Submit),
                Enqueue::User(UserAction::Esc),
            ],
            expected: vec![
                TransportItem::User(UserAction::InsertChar('a')),
                TransportItem::User(UserAction::Submit),
                TransportItem::User(UserAction::Esc),
            ],
        },
        Case {
            name: "fifo for ui-event source only",
            enqueue: vec![
                Enqueue::Event(Box::new(completed(1))),
                Enqueue::Event(Box::new(warning("warn-1"))),
                Enqueue::Event(Box::new(completed(2))),
            ],
            expected: vec![
                TransportItem::Event(Box::new(completed(1))),
                TransportItem::Event(Box::new(warning("warn-1"))),
                TransportItem::Event(Box::new(completed(2))),
            ],
        },
        Case {
            name: "both non-empty alternates by source, starts with user",
            enqueue: vec![
                Enqueue::User(UserAction::InsertChar('u')),
                Enqueue::User(UserAction::InsertChar('v')),
                Enqueue::Event(Box::new(warning("e1"))),
                Enqueue::Event(Box::new(completed(9))),
            ],
            expected: vec![
                TransportItem::User(UserAction::InsertChar('u')),
                TransportItem::Event(Box::new(warning("e1"))),
                TransportItem::User(UserAction::InsertChar('v')),
                TransportItem::Event(Box::new(completed(9))),
            ],
        },
        Case {
            name: "when one side empties, remaining side drains in fifo",
            enqueue: vec![
                Enqueue::User(UserAction::Submit),
                Enqueue::Event(Box::new(warning("e1"))),
                Enqueue::Event(Box::new(completed(3))),
            ],
            expected: vec![
                TransportItem::User(UserAction::Submit),
                TransportItem::Event(Box::new(warning("e1"))),
                TransportItem::Event(Box::new(completed(3))),
            ],
        },
    ];

    for case in cases {
        let mut transport = TuiTransport::new();

        for item in case.enqueue {
            match item {
                Enqueue::User(action) => transport.enqueue_user_action(action),
                Enqueue::Event(event) => transport.enqueue_ui_event(*event),
            }
        }

        let actual = drain(&mut transport);
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
        assert!(
            transport.is_empty(),
            "transport should be empty: {}",
            case.name
        );
    }
}

#[test]
fn explicit_unbounded_policy_accepts_large_burst_without_drops() {
    let mut transport = TuiTransport::new();
    let total = 10_000usize;

    for _ in 0..total {
        transport.enqueue_user_action(UserAction::InsertChar('u'));
    }

    let drained = drain(&mut transport);
    assert_eq!(drained.len(), total);
    assert_eq!(
        drained.first(),
        Some(&TransportItem::User(UserAction::InsertChar('u')))
    );
    assert_eq!(
        drained.last(),
        Some(&TransportItem::User(UserAction::InsertChar('u')))
    );
}
