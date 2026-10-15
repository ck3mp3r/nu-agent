use super::*;

#[test]
fn circuit_breaker_sequences_table() {
    // -- Setup & Fixtures
    enum Op {
        Fail(&'static str),
        Succeed(&'static str),
    }

    struct Case {
        name: &'static str,
        threshold: usize,
        ops: Vec<Op>,
        expected_trips: Vec<bool>,
        expected_counts: Vec<(&'static str, usize)>,
    }

    let cases = vec![
        Case {
            name: "trips after threshold",
            threshold: 3,
            ops: vec![
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
            ],
            expected_trips: vec![false, false, true],
            expected_counts: vec![("server_a", 3)],
        },
        Case {
            name: "resets on success",
            threshold: 3,
            ops: vec![
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Succeed("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
            ],
            expected_trips: vec![false, false, false, false, false],
            expected_counts: vec![("server_a", 2)],
        },
        Case {
            name: "independent per server",
            threshold: 3,
            ops: vec![
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_b"),
                Op::Fail("server_b"),
                Op::Fail("server_b"),
            ],
            expected_trips: vec![false, false, false, false, true],
            expected_counts: vec![("server_a", 2), ("server_b", 3)],
        },
        Case {
            name: "trips stay tripped on further failures",
            threshold: 3,
            ops: vec![
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
            ],
            expected_trips: vec![false, false, true, true, true],
            expected_counts: vec![("server_a", 5)],
        },
        Case {
            name: "custom threshold is respected",
            threshold: 5,
            ops: vec![
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
                Op::Fail("server_a"),
            ],
            expected_trips: vec![false, false, false, false, true],
            expected_counts: vec![("server_a", 5)],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let mut cb = McpCircuitBreaker::new(case.threshold);
        for (op, expected_trip) in case.ops.iter().zip(&case.expected_trips) {
            let tripped = match op {
                Op::Fail(server) => cb.record_failure(server),
                Op::Succeed(server) => {
                    cb.record_success(server);
                    false
                }
            };
            assert_eq!(tripped, *expected_trip, "case '{}': trip result", case.name);
        }
        for (server, count) in &case.expected_counts {
            assert_eq!(
                cb.failure_counts.get(*server).copied(),
                Some(*count),
                "case '{}': failure count for {server}",
                case.name
            );
        }
    }
}

#[test]
fn circuit_breaker_threshold_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Option<usize>, usize)> = vec![
        ("default constructor", None, 10),
        ("zero threshold uses default", Some(0), 10),
    ];

    // -- Exec & Check
    for (name, configured, expected) in cases {
        let cb = match configured {
            Some(threshold) => McpCircuitBreaker::new(threshold),
            None => McpCircuitBreaker::default(),
        };
        assert_eq!(cb.threshold(), expected, "case '{name}'");
    }
}
