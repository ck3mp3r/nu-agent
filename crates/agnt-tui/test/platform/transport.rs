use crate::platform::transport::{Source, TransportItem, TuiTransport};
use agnt_core::protocol::events::{Key, SystemEvent, SystemKind};

#[derive(Clone)]
enum Enqueue {
    Input(SystemEvent),
    Event(SystemEvent),
}

fn key(ch: char) -> SystemEvent {
    SystemEvent::KeyPressed { key: Key::Char(ch) }
}

fn completed(tool_calls: usize) -> SystemEvent {
    SystemEvent::TurnCompleted {
        tool_calls,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    }
}

fn warning(msg: &str) -> SystemEvent {
    SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: msg.to_string(),
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
    let mut transport = TuiTransport::default();

    assert!(transport.poll_next().is_none());
    assert!(transport.is_empty());
}

#[test]
fn table_driven_fifo_and_deterministic_merge_policy_round_robin_input_first() {
    struct Case {
        name: &'static str,
        enqueue: Vec<Enqueue>,
        expected: Vec<TransportItem>,
    }

    let cases = vec![
        Case {
            name: "fifo for input source only",
            enqueue: vec![
                Enqueue::Input(key('a')),
                Enqueue::Input(key('b')),
                Enqueue::Input(key('c')),
            ],
            expected: vec![
                TransportItem {
                    source: Source::Input,
                    event: key('a'),
                },
                TransportItem {
                    source: Source::Input,
                    event: key('b'),
                },
                TransportItem {
                    source: Source::Input,
                    event: key('c'),
                },
            ],
        },
        Case {
            name: "fifo for event source only",
            enqueue: vec![
                Enqueue::Event(completed(1)),
                Enqueue::Event(warning("warn-1")),
                Enqueue::Event(completed(2)),
            ],
            expected: vec![
                TransportItem {
                    source: Source::Event,
                    event: completed(1),
                },
                TransportItem {
                    source: Source::Event,
                    event: warning("warn-1"),
                },
                TransportItem {
                    source: Source::Event,
                    event: completed(2),
                },
            ],
        },
        Case {
            name: "both non-empty alternates by source, starts with input",
            enqueue: vec![
                Enqueue::Input(key('u')),
                Enqueue::Input(key('v')),
                Enqueue::Event(warning("e1")),
                Enqueue::Event(completed(9)),
            ],
            expected: vec![
                TransportItem {
                    source: Source::Input,
                    event: key('u'),
                },
                TransportItem {
                    source: Source::Event,
                    event: warning("e1"),
                },
                TransportItem {
                    source: Source::Input,
                    event: key('v'),
                },
                TransportItem {
                    source: Source::Event,
                    event: completed(9),
                },
            ],
        },
        Case {
            name: "when one side empties, remaining side drains in fifo",
            enqueue: vec![
                Enqueue::Input(key('a')),
                Enqueue::Event(warning("e1")),
                Enqueue::Event(completed(3)),
            ],
            expected: vec![
                TransportItem {
                    source: Source::Input,
                    event: key('a'),
                },
                TransportItem {
                    source: Source::Event,
                    event: warning("e1"),
                },
                TransportItem {
                    source: Source::Event,
                    event: completed(3),
                },
            ],
        },
    ];

    for case in cases {
        let mut transport = TuiTransport::default();

        for item in case.enqueue {
            match item {
                Enqueue::Input(event) => transport.enqueue_input(event),
                Enqueue::Event(event) => transport.enqueue_event(event),
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
    let mut transport = TuiTransport::default();
    let total = 10_000usize;

    for _ in 0..total {
        transport.enqueue_input(key('u'));
    }

    let drained = drain(&mut transport);
    assert_eq!(drained.len(), total);
    assert_eq!(
        drained.first(),
        Some(&TransportItem {
            source: Source::Input,
            event: key('u')
        })
    );
    assert_eq!(
        drained.last(),
        Some(&TransportItem {
            source: Source::Input,
            event: key('u')
        })
    );
}
