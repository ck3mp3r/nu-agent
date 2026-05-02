use nu_plugin::EvaluatedCall;
use nu_protocol::{Span, Spanned, Value};

use crate::commands::agent::ui::policy::{Verbosity, resolve_ui_policy};

fn call_with_flags(flags: Vec<(&str, Option<Value>)>) -> EvaluatedCall {
    let head = Span::test_data();
    let named = flags
        .into_iter()
        .map(|(name, value)| {
            (
                Spanned {
                    item: name.to_string(),
                    span: head,
                },
                value,
            )
        })
        .collect();

    EvaluatedCall {
        head,
        positional: vec![],
        named,
    }
}

#[test]
fn default_policy_is_normal() {
    let policy = resolve_ui_policy(&call_with_flags(vec![])).expect("policy");
    assert_eq!(policy.verbosity, Verbosity::Normal);
    assert!(!policy.quiet);
}

#[test]
fn quiet_overrides_verbose_levels() {
    let policy = resolve_ui_policy(&call_with_flags(vec![
        ("quiet", None),
        ("v", None),
        ("v", None),
        ("verbose", None),
    ]))
    .expect("policy");

    assert!(policy.quiet);
    assert_eq!(policy.verbosity, Verbosity::Quiet);
}

#[test]
fn repeated_v_increases_verbosity_progressively() {
    let v1 = resolve_ui_policy(&call_with_flags(vec![("v", None)])).expect("v1");
    assert_eq!(v1.verbosity, Verbosity::Verbose);

    let v2 = resolve_ui_policy(&call_with_flags(vec![("v", None), ("v", None)])).expect("v2");
    assert_eq!(v2.verbosity, Verbosity::VeryVerbose);

    let v3 = resolve_ui_policy(&call_with_flags(vec![("v", None), ("v", None), ("v", None)]))
        .expect("v3");
    assert_eq!(v3.verbosity, Verbosity::Trace);
}
