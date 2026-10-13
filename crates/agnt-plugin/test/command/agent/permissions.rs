use super::test_helpers::create_test_call;
use agnt_core::tools::authz::PermissionAction;
use nu_protocol::{Value, record};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Tool-call arguments as rig 0.44 carries them: a JSON object map.
macro_rules! args_map {
    ($($json:tt)*) => {
        serde_json::json!($($json)*)
            .as_object()
            .cloned()
            .unwrap_or_default()
    };
}

#[test]
fn resolve_effective_permissions_merges_cli_overlay_additively() -> Result<()> {
    let plugin_config = agnt_core::config::PluginConfig {
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
            .map_err(|e| format!("{e:?}"))?;

    assert_eq!(
        effective.evaluate("read", &args_map!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        effective
            .evaluate("nu", &args_map!({"command": "kubectl get pods"}))
            .action,
        PermissionAction::Allow
    );
    assert_eq!(
        effective
            .evaluate("nu", &args_map!({"command": "kubectl delete pod x"}))
            .action,
        PermissionAction::Deny
    );
    assert!(summary.contains("overlay_active=true"));
    Ok(())
}
