use super::*;
use crate::tools::closure::{ClosureParameter, ResolvedClosure};
use nu_protocol::{BlockId, Span, Spanned, engine::Closure};

fn create_test_closure() -> Spanned<Closure> {
    Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::unknown(),
    }
}

fn create_test_resolved() -> ResolvedClosure {
    ResolvedClosure {
        closure: create_test_closure(),
        params: vec![],
    }
}

#[test]
fn registry_register_and_names_table() {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        registrations: Vec<&'static str>,
        expected_names: Vec<&'static str>,
    }

    let cases = vec![
        Case {
            name: "new registry is empty",
            registrations: vec![],
            expected_names: vec![],
        },
        Case {
            name: "register adds a closure",
            registrations: vec!["add"],
            expected_names: vec!["add"],
        },
        Case {
            name: "register multiple closures",
            registrations: vec!["add", "multiply", "divide"],
            expected_names: vec!["add", "multiply", "divide"],
        },
        Case {
            name: "register overwrites an existing name",
            registrations: vec!["add", "add"],
            expected_names: vec!["add"],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let mut registry = ClosureRegistry::default();
        for name in &case.registrations {
            registry.register(name.to_string(), create_test_resolved());
        }

        let names: Vec<&String> = registry.names().collect();
        assert_eq!(
            names.len(),
            case.expected_names.len(),
            "case '{}': name count",
            case.name
        );
        for expected in &case.expected_names {
            assert!(
                names.contains(&&expected.to_string()),
                "case '{}': names must contain {expected}, got {names:?}",
                case.name
            );
            assert!(
                registry.get(expected).is_some(),
                "case '{}': get({expected}) must resolve",
                case.name
            );
        }
    }
}

#[test]
fn register_stores_resolved_closure_with_params() {
    let mut reg = ClosureRegistry::default();
    let resolved = ResolvedClosure {
        closure: create_test_closure(),
        params: vec![ClosureParameter {
            name: "x".to_string(),
            is_required: true,
        }],
    };
    reg.register("tool".to_string(), resolved);
    let got = reg.get("tool").unwrap();
    assert_eq!(got.params.len(), 1);
    assert_eq!(got.params[0].name, "x");
}
