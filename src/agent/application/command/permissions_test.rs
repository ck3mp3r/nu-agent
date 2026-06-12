use crate::agent::application::command::test_helpers::create_test_call;
use crate::agent::tools::authz::{NonInteractiveAskMode, PermissionAction};
use nu_protocol::{Value, record};

#[test]
fn resolve_non_interactive_ask_mode_defaults_to_deny_when_missing() {
    let mode = super::resolve_non_interactive_ask_mode(None).expect("mode");
    assert_eq!(mode, NonInteractiveAskMode::Deny);
}

#[test]
fn resolve_non_interactive_ask_mode_rejects_invalid_value() {
    let invalid = Value::test_record(record! {
        "non_interactive_ask" => Value::test_string("ask")
    });
    let error = super::resolve_non_interactive_ask_mode(Some(&invalid))
        .expect_err("invalid value should fail");
    assert!(error.msg.contains("Invalid non_interactive_ask value"));
}

#[test]
fn resolve_effective_permissions_merges_cli_overlay_additively() {
    let plugin = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "read" => Value::test_string("allow"),
            "nu__run" => Value::test_record(record! {
                "command" => Value::test_record(record! {
                    "kubectl get *" => Value::test_string("allow"),
                    "*" => Value::test_string("ask")
                })
            })
        })
    });
    let call = create_test_call(vec![(
        "permissions",
        Value::test_record(record! {
            "read" => Value::test_string("deny"),
            "nu__run" => Value::test_record(record! {
                "command" => Value::test_record(record! {
                    "kubectl delete *" => Value::test_string("deny")
                })
            })
        }),
    )]);

    let (effective, summary) =
        super::resolve_effective_permissions_config(&call, Some(&plugin), None, true)
            .expect("merge");

    assert_eq!(
        effective.evaluate("read", &serde_json::json!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        effective
            .evaluate(
                "nu__run",
                &serde_json::json!({"command": "kubectl get pods"})
            )
            .action,
        PermissionAction::Allow
    );
    assert_eq!(
        effective
            .evaluate(
                "nu__run",
                &serde_json::json!({"command": "kubectl delete pod x"})
            )
            .action,
        PermissionAction::Deny
    );
    assert!(summary.contains("overlay_active=true"));
}

#[test]
fn resolve_effective_permissions_rejects_malformed_cli_with_path_diagnostic() {
    let call = create_test_call(vec![(
        "permissions",
        Value::test_record(record! {
            "nu__run" => Value::test_record(record! {
                "argv" => Value::test_record(record! {
                    "*" => Value::test_string("deny")
                })
            })
        }),
    )]);

    let err = super::resolve_effective_permissions_config(&call, None, None, true)
        .expect_err("malformed cli permissions must fail fast");

    assert!(err.msg.contains("Invalid --permissions value"));
}
