use super::test_helpers::create_test_call;
use nu_agent_core::tools::authz::PermissionAction;
use nu_protocol::{Value, record};
#[test]
fn resolve_effective_permissions_merges_cli_overlay_additively() {
    let plugin_config = nu_agent_core::config::PluginConfig {
        permissions: Some(
            toml::from_str(
                r#"
"*" = "ask"
read = "allow"

[nu.command]
"kubectl get *" = "allow"
"*" = "ask"
"#,
            )
            .unwrap(),
        ),
        ..Default::default()
    };
    let call = create_test_call(vec![(
        "permissions",
        Value::test_record(record! {
            "read" => Value::test_string("deny"),
            "nu" => Value::test_record(record! {
                "command" => Value::test_record(record! {
                    "kubectl delete *" => Value::test_string("deny")
                })
            })
        }),
    )]);

    let (_base, effective, _cli_overlay, summary) =
        super::permissions::resolve_effective_permissions_config(&call, &plugin_config, None, true)
            .expect("merge");

    assert_eq!(
        effective.evaluate("read", &serde_json::json!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        effective
            .evaluate("nu", &serde_json::json!({"command": "kubectl get pods"}))
            .action,
        PermissionAction::Allow
    );
    assert_eq!(
        effective
            .evaluate(
                "nu",
                &serde_json::json!({"command": "kubectl delete pod x"})
            )
            .action,
        PermissionAction::Deny
    );
    assert!(summary.contains("overlay_active=true"));
}
