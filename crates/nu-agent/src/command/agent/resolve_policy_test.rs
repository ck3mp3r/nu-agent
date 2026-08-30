use nu_plugin::EvaluatedCall;
use nu_protocol::{Span, Spanned, Value};

use super::resolve_policy::resolve_ui_policy;
use nu_agent_tty::policy::Verbosity;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
fn default_policy_is_normal() -> Result<()> {
    let policy = resolve_ui_policy(&call_with_flags(vec![])).map_err(|e| format!("{e:?}"))?;
    assert_eq!(policy.verbosity, Verbosity::Normal);
    assert!(!policy.quiet);
    Ok(())
}

#[test]
fn quiet_overrides_verbose_levels() -> Result<()> {
    let policy = resolve_ui_policy(&call_with_flags(vec![
        ("quiet", None),
        ("v", None),
        ("v", None),
        ("verbose", None),
    ]))
    .map_err(|e| format!("{e:?}"))?;

    assert!(policy.quiet);
    assert_eq!(policy.verbosity, Verbosity::Quiet);
    Ok(())
}

#[test]
fn repeated_v_increases_verbosity_progressively() -> Result<()> {
    let v1 =
        resolve_ui_policy(&call_with_flags(vec![("v", None)])).map_err(|e| format!("{e:?}"))?;
    assert_eq!(v1.verbosity, Verbosity::Verbose);

    let v2 = resolve_ui_policy(&call_with_flags(vec![("v", None), ("v", None)]))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(v2.verbosity, Verbosity::VeryVerbose);

    let v3 = resolve_ui_policy(&call_with_flags(vec![
        ("v", None),
        ("v", None),
        ("v", None),
    ]))
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(v3.verbosity, Verbosity::Trace);
    Ok(())
}
