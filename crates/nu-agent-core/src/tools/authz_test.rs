use super::*;
use nu_protocol::{Value, record};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::protocol::event::{
    PermissionDecision as UiPermissionDecision, PermissionDecisionSubmission, UiEvent,
};

fn ask_decision_fixture() -> PermissionDecision {
    PermissionDecision {
        action: PermissionAction::Ask,
        matched_rule: PermissionRuleMatch {
            identity: "nested:nu__run.command:*".to_string(),
            scope: "nested",
            target_field: Some("command"),
            pattern: "*".to_string(),
            action: PermissionAction::Ask,
        },
        diagnostics: Vec::new(),
    }
}

struct ChannelPermissionSink {
    tx: mpsc::Sender<UiEvent>,
}

impl PermissionEventSink for ChannelPermissionSink {
    fn emit(&mut self, event: UiEvent) {
        let _ = self.tx.send(event);
    }
}

#[derive(Default)]
struct RecordingPermissionSink {
    events: Vec<UiEvent>,
}

impl PermissionEventSink for RecordingPermissionSink {
    fn emit(&mut self, event: UiEvent) {
        self.events.push(event);
    }
}

fn permissions_value() -> nu_protocol::Value {
    Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "read" => Value::test_string("allow"),
            "c5t_get*" => Value::test_string("allow"),
            "nu__run" => Value::test_record(record! {
                "command" => Value::test_record(record! {
                    "kubectl delete *" => Value::test_string("deny"),
                    "*" => Value::test_string("ask"),
                })
            })
        })
    })
}

#[test]
fn parser_accepts_canonical_permissions_shape() {
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&permissions_value()), true);
    let deny = parsed.evaluate(
        "nu__run",
        &serde_json::json!({"command": "kubectl delete pod x"}),
    );
    assert_eq!(deny.action, PermissionAction::Deny);
    assert_eq!(deny.matched_rule.scope, "nested");
    assert_eq!(deny.matched_rule.target_field, Some("command"));
}

#[test]
fn precedence_is_global_then_tool_then_nested_command() {
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&permissions_value()), true);

    let global = parsed.evaluate("unknown_tool", &serde_json::json!({}));
    assert_eq!(global.action, PermissionAction::Ask);
    assert_eq!(global.matched_rule.scope, "global");

    let tool = parsed.evaluate("read", &serde_json::json!({}));
    assert_eq!(tool.action, PermissionAction::Allow);
    assert_eq!(tool.matched_rule.scope, "tool");

    let nested = parsed.evaluate(
        "nu__run",
        &serde_json::json!({"command": "kubectl delete ns prod"}),
    );
    assert_eq!(nested.action, PermissionAction::Deny);
    assert_eq!(nested.matched_rule.scope, "nested");
}

#[test]
fn nu_run_command_matching_normalizes_whitespace() {
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&permissions_value()), true);
    let decision = parsed.evaluate(
        "nu__run",
        &serde_json::json!({"command": "   kubectl    delete   pod   foo   "}),
    );
    assert_eq!(decision.action, PermissionAction::Deny);
}

#[test]
fn missing_command_uses_deterministic_safe_fallback_with_diagnostics() {
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&permissions_value()), true);
    let decision = parsed.evaluate("nu__run", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Ask);
    assert!(
        decision
            .diagnostics
            .iter()
            .any(|diag| diag.code == "permissions.nu_run.command.missing")
    );
}

#[test]
fn unknown_nu_run_nested_field_is_rejected_with_diagnostic() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "nu__run" => Value::test_record(record! {
                "args" => Value::test_record(record! {
                    "*" => Value::test_string("deny")
                })
            })
        })
    });

    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|diag| diag.code == "permissions.invalid.nu_run_field")
    );
}

#[test]
fn redundant_nested_star_equal_to_inherited_is_valid_noop_with_diagnostic() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "nu__run" => Value::test_record(record! {
                "command" => Value::test_record(record! {
                    "*" => Value::test_string("ask")
                })
            })
        })
    });

    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    let decision = parsed.evaluate("nu__run", &serde_json::json!({"command": "echo hi"}));

    assert_eq!(decision.action, PermissionAction::Ask);
    assert!(
        decision
            .diagnostics
            .iter()
            .any(|diag| diag.code == "permissions.noop.nu_run.command.star")
    );
}

#[test]
fn ask_choices_apply_once_always_and_deny() {
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&permissions_value()), true);
    let mut cache = SessionGrantCache::default();
    let args = serde_json::json!({"command": "echo hi"});

    let base = parsed.evaluate("nu__run", &args);
    assert_eq!(base.action, PermissionAction::Ask);

    let once = apply_ask_choice(
        base.clone(),
        AskChoice::AllowOnce,
        &mut cache,
        "nu__run",
        "closure",
        &args,
    );
    assert_eq!(once.action, PermissionAction::Allow);
    assert!(cache.get(&base, "nu__run", "closure", &args).is_none());

    let always = apply_ask_choice(
        base.clone(),
        AskChoice::AllowAlways,
        &mut cache,
        "nu__run",
        "closure",
        &args,
    );
    assert_eq!(always.action, PermissionAction::Allow);
    assert_eq!(
        cache.get(&base, "nu__run", "closure", &args),
        Some(PermissionAction::Allow)
    );

    let denied = apply_ask_choice(
        base.clone(),
        AskChoice::Deny,
        &mut cache,
        "nu__run",
        "closure",
        &args,
    );
    assert_eq!(denied.action, PermissionAction::Deny);
}

#[test]
fn session_grants_are_keyed_by_scoped_request_context_not_call_arguments() {
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&permissions_value()), true);
    let mut cache = SessionGrantCache::default();
    let first_args = serde_json::json!({"command": "echo one"});
    let second_args = serde_json::json!({"command": "echo two"});

    let first = parsed.evaluate("nu__run", &first_args);
    let second = parsed.evaluate("nu__run", &second_args);
    assert_eq!(first.matched_rule.identity, second.matched_rule.identity);

    let _ = apply_ask_choice(
        first.clone(),
        AskChoice::AllowAlways,
        &mut cache,
        "nu__run",
        "closure",
        &first_args,
    );

    let overridden =
        apply_session_grant_override(second, &cache, "nu__run", "closure", &second_args);
    assert_eq!(overridden.action, PermissionAction::Allow);
}

#[test]
fn allow_always_for_nu_run_does_not_leak_to_read_under_global_ask() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask")
        })
    });
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    let mut cache = SessionGrantCache::default();

    let nu_run_args = serde_json::json!({"command": "echo one"});
    let nu_run = parsed.evaluate("nu__run", &nu_run_args);
    let _ = apply_ask_choice(
        nu_run,
        AskChoice::AllowAlways,
        &mut cache,
        "nu__run",
        "closure",
        &nu_run_args,
    );

    let read_args = serde_json::json!({"filePath": "README.md"});
    let read = parsed.evaluate("read", &read_args);
    assert_eq!(read.matched_rule.identity, "global:*");
    let overridden = apply_session_grant_override(read, &cache, "read", "closure", &read_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_rule_identity_different_tool_name_does_not_share_session_grant() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask")
        })
    });
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    let mut cache = SessionGrantCache::default();

    let glob_args = serde_json::json!({"pattern": "**/*.rs"});
    let glob = parsed.evaluate("glob", &glob_args);
    let _ = apply_ask_choice(
        glob,
        AskChoice::AllowAlways,
        &mut cache,
        "glob",
        "closure",
        &glob_args,
    );

    let read_args = serde_json::json!({"filePath": "README.md"});
    let read = parsed.evaluate("read", &read_args);
    assert_eq!(read.matched_rule.identity, "global:*");
    let overridden = apply_session_grant_override(read, &cache, "read", "closure", &read_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_tool_name_different_mode_does_not_share_session_grant() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "edit" => Value::test_string("ask")
        })
    });
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    let mut cache = SessionGrantCache::default();

    let preview_args = serde_json::json!({
        "path": "file.txt",
        "mode": "preview"
    });
    let preview = parsed.evaluate("edit", &preview_args);
    let _ = apply_ask_choice(
        preview,
        AskChoice::AllowAlways,
        &mut cache,
        "edit",
        "closure",
        &preview_args,
    );

    let apply_args = serde_json::json!({
        "path": "file.txt",
        "mode": "apply"
    });
    let apply = parsed.evaluate("edit", &apply_args);
    let overridden = apply_session_grant_override(apply, &cache, "edit", "closure", &apply_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_tool_name_same_mode_different_source_does_not_share_session_grant() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "edit" => Value::test_string("ask")
        })
    });
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    let mut cache = SessionGrantCache::default();

    let args = serde_json::json!({
        "path": "file.txt",
        "mode": "apply"
    });
    let closure_decision = parsed.evaluate("edit", &args);
    let _ = apply_ask_choice(
        closure_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "edit",
        "closure",
        &args,
    );

    let mcp_decision = parsed.evaluate("edit", &args);
    let overridden = apply_session_grant_override(mcp_decision, &cache, "edit", "mcp", &args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_tool_source_mode_different_rule_identity_does_not_share_session_grant() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "nu__run" => Value::test_record(record! {
                "command" => Value::test_record(record! {
                    "echo *" => Value::test_string("ask"),
                    "ls *" => Value::test_string("ask")
                })
            })
        })
    });
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    let mut cache = SessionGrantCache::default();

    let echo_args = serde_json::json!({"command": "echo one"});
    let echo_decision = parsed.evaluate("nu__run", &echo_args);
    assert_eq!(
        echo_decision.matched_rule.identity,
        "nested:nu__run.command:echo *"
    );
    let _ = apply_ask_choice(
        echo_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "nu__run",
        "closure",
        &echo_args,
    );

    let ls_args = serde_json::json!({"command": "ls -la"});
    let ls_decision = parsed.evaluate("nu__run", &ls_args);
    assert_eq!(
        ls_decision.matched_rule.identity,
        "nested:nu__run.command:ls *"
    );
    let overridden =
        apply_session_grant_override(ls_decision, &cache, "nu__run", "closure", &ls_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn defaults_apply_when_permissions_block_is_missing() {
    let parsed = PermissionsConfig::parse_from_plugin_config(None, true);
    assert_eq!(
        parsed.evaluate("read", &serde_json::json!({})).action,
        PermissionAction::Allow
    );
    assert_eq!(
        parsed
            .evaluate(
                "nu__run",
                &serde_json::json!({"command": "kubectl get pods"})
            )
            .action,
        PermissionAction::Ask
    );
}

#[test]
#[serial_test::serial]
fn async_ask_waits_for_matching_decision_before_resolving() {
    crate::protocol::permission::install_active_permission_submission_sender(None);
    let mut hook = AsyncAskHook::new(AskRuntimeConfig {
        interactive: true,
        non_interactive_mode: NonInteractiveAskMode::Deny,
        timeout: Duration::from_secs(2),
    });
    let decision = ask_decision_fixture();
    let args = serde_json::json!({"command": "echo hi"});

    let (event_tx, event_rx) = mpsc::channel::<UiEvent>();
    let (choice_tx, choice_rx) = mpsc::channel::<AskChoice>();

    let handle = thread::spawn(move || {
        let mut sink = ChannelPermissionSink { tx: event_tx };
        let choice = hook.choose_with_sink(
            &decision,
            "nu__run",
            "closure",
            &args,
            &AskContext::default(),
            Some(&mut sink),
        );
        let _ = choice_tx.send(choice);
    });

    let (request_id, rule_identity) = match event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("permission request event")
    {
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => {
            assert_eq!(context.tool, "nu__run(command=echo hi)");
            assert!(context.pre_authorize_display.is_none());
            (request_id, context.matched_rule_identity)
        }
        other => panic!("unexpected event: {other:?}"),
    };

    assert!(
        choice_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "ask must block until a decision is submitted"
    );

    let outcome = crate::protocol::permission::submit_active_permission_decision(
        request_id.clone(),
        UiPermissionDecision::AllowOnce,
        rule_identity,
    );
    assert_eq!(
        outcome,
        crate::protocol::permission::SubmitOutcome::Accepted
    );

    let choice = choice_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("resolved choice");
    assert_eq!(choice, AskChoice::AllowOnce);

    let submitted = event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("submitted event");
    match submitted {
        UiEvent::PermissionDecisionSubmitted {
            request_id: submitted_id,
            decision,
            ..
        } => {
            assert_eq!(submitted_id, request_id);
            assert_eq!(decision, UiPermissionDecision::AllowOnce);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    handle.join().expect("ask thread join");
}

#[test]
#[serial_test::serial]
fn ask_event_carries_pre_authorize_display_context_when_provided() {
    crate::protocol::permission::install_active_permission_submission_sender(None);
    let mut hook = AsyncAskHook::new(AskRuntimeConfig {
        interactive: true,
        non_interactive_mode: NonInteractiveAskMode::Deny,
        timeout: Duration::from_secs(2),
    });
    let decision = ask_decision_fixture();
    let args = serde_json::json!({"path": "file.txt", "mode": "apply"});

    let ask_context = AskContext {
        pre_authorize_display: Some(crate::protocol::event::ToolDisplay {
            title: "edit file.txt".to_string(),
            sections: vec![crate::protocol::event::ToolDisplaySection {
                label: "file.txt".to_string(),
                language: "diff".to_string(),
                content: "--- a/file.txt\n+++ b/file.txt\n".to_string(),
                stats: None,
            }],
        }),
    };

    let (event_tx, event_rx) = mpsc::channel::<UiEvent>();
    let (choice_tx, choice_rx) = mpsc::channel::<AskChoice>();

    let handle = thread::spawn(move || {
        let mut sink = ChannelPermissionSink { tx: event_tx };
        let choice =
            hook.choose_with_sink(&decision, "edit", "closure", &args, &ask_context, Some(&mut sink));
        let _ = choice_tx.send(choice);
    });

    let request = event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("permission request event");
    let (request_id, rule_identity) = match request {
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => {
            let display = context
                .pre_authorize_display
                .expect("pre_authorize display must be propagated");
            assert_eq!(display.title, "edit file.txt");
            assert_eq!(display.sections.len(), 1);
            assert_eq!(display.sections[0].language, "diff");
            (request_id, context.matched_rule_identity)
        }
        other => panic!("unexpected event: {other:?}"),
    };

    let outcome = crate::protocol::permission::submit_active_permission_decision(
        request_id.clone(),
        UiPermissionDecision::AllowOnce,
        rule_identity,
    );
    assert_eq!(
        outcome,
        crate::protocol::permission::SubmitOutcome::Accepted
    );
    let choice = choice_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("resolved choice");
    assert_eq!(choice, AskChoice::AllowOnce);
    handle.join().expect("ask thread join");
}

#[test]
#[serial_test::serial]
fn async_ask_timeout_is_deterministic_deny() {
    crate::protocol::permission::install_active_permission_submission_sender(None);
    let mut hook = AsyncAskHook::new(AskRuntimeConfig {
        interactive: true,
        non_interactive_mode: NonInteractiveAskMode::Allow,
        timeout: Duration::from_millis(30),
    });
    let decision = ask_decision_fixture();
    let args = serde_json::json!({"command": "echo timeout"});
    let mut sink = RecordingPermissionSink::default();

    let choice = hook.choose_with_sink(
        &decision,
        "nu__run",
        "closure",
        &args,
        &AskContext::default(),
        Some(&mut sink),
    );
    assert_eq!(choice, AskChoice::Deny);
    assert!(
        sink.events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionDecisionTimedOut { .. }))
    );
}

#[test]
#[serial_test::serial]
fn non_interactive_ask_defaults_to_deny() {
    crate::protocol::permission::install_active_permission_submission_sender(None);
    let mut hook = AsyncAskHook::new(AskRuntimeConfig {
        interactive: false,
        non_interactive_mode: NonInteractiveAskMode::Deny,
        timeout: Duration::from_secs(1),
    });
    let decision = ask_decision_fixture();
    let mut sink = RecordingPermissionSink::default();

    let choice = hook.choose_with_sink(
        &decision,
        "nu__run",
        "closure",
        &serde_json::json!({"command": "echo denied"}),
        &AskContext::default(),
        Some(&mut sink),
    );
    assert_eq!(choice, AskChoice::Deny);
    assert!(sink.events.is_empty());
}

#[test]
#[serial_test::serial]
fn non_interactive_ask_allow_override_returns_allow_once() {
    crate::protocol::permission::install_active_permission_submission_sender(None);
    let mut hook = AsyncAskHook::new(AskRuntimeConfig {
        interactive: false,
        non_interactive_mode: NonInteractiveAskMode::Allow,
        timeout: Duration::from_secs(1),
    });
    let decision = ask_decision_fixture();
    let mut sink = RecordingPermissionSink::default();

    let choice = hook.choose_with_sink(
        &decision,
        "nu__run",
        "closure",
        &serde_json::json!({"command": "echo allowed"}),
        &AskContext::default(),
        Some(&mut sink),
    );
    assert_eq!(choice, AskChoice::AllowOnce);
    assert!(sink.events.is_empty());
}

#[test]
fn allow_always_grant_is_session_only_and_resets_with_new_cache() {
    let parsed = PermissionsConfig::parse_from_plugin_config(Some(&permissions_value()), true);

    let mut first_session_cache = SessionGrantCache::default();
    let first_args = serde_json::json!({"command": "echo one"});
    let first = parsed.evaluate("nu__run", &first_args);
    let _ = apply_ask_choice(
        first.clone(),
        AskChoice::AllowAlways,
        &mut first_session_cache,
        "nu__run",
        "closure",
        &first_args,
    );
    let first_overridden = apply_session_grant_override(
        first.clone(),
        &first_session_cache,
        "nu__run",
        "closure",
        &first_args,
    );
    assert_eq!(first_overridden.action, PermissionAction::Allow);

    let fresh_session_cache = SessionGrantCache::default();
    let second_args = serde_json::json!({"command": "echo two"});
    let second = parsed.evaluate("nu__run", &second_args);
    let second_overridden = apply_session_grant_override(
        second,
        &fresh_session_cache,
        "nu__run",
        "closure",
        &second_args,
    );
    assert_eq!(second_overridden.action, PermissionAction::Ask);
}

#[test]
fn cli_permissions_overlay_rejects_non_record_with_explicit_path() {
    let err = PermissionsOverlay::parse_from_cli_value(&Value::test_string("ask"))
        .expect_err("non-record --permissions must fail");
    assert!(err.contains("permissions"));
    assert!(err.contains("record"));
}

#[test]
fn cli_permissions_overlay_rejects_unknown_nested_field_with_path() {
    let err = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "nu__run" => Value::test_record(record! {
            "argv" => Value::test_record(record! {
                "*" => Value::test_string("deny")
            })
        })
    }))
    .expect_err("unknown nested field must fail");

    assert!(err.contains("permissions.nu__run.argv"));
}

#[test]
#[serial_test::serial]
fn async_ask_waits_for_matching_decision_before_resolving_via_trait() {
    crate::protocol::permission::install_active_permission_submission_sender(None);
    let mut hook = AsyncAskHook::new(AskRuntimeConfig {
        interactive: true,
        non_interactive_mode: NonInteractiveAskMode::Deny,
        timeout: Duration::from_secs(2),
    });
    let decision = ask_decision_fixture();
    let args = serde_json::json!({"command": "echo hi"});

    let (event_tx, event_rx) = mpsc::channel::<UiEvent>();
    let (choice_tx, choice_rx) = mpsc::channel::<AskChoice>();

    let handle = thread::spawn(move || {
        let mut sink = ChannelPermissionSink { tx: event_tx };
        // Call through the trait method instead of the inherent method.
        let choice = AskApprovalHook::choose(
            &mut hook,
            &decision,
            "nu__run",
            "closure",
            &args,
            &AskContext::default(),
            Some(&mut sink),
        );
        let _ = choice_tx.send(choice);
    });

    let (request_id, rule_identity) = match event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("permission request event")
    {
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => {
            assert_eq!(context.tool, "nu__run(command=echo hi)");
            assert!(context.pre_authorize_display.is_none());
            (request_id, context.matched_rule_identity)
        }
        other => panic!("unexpected event: {other:?}"),
    };

    assert!(
        choice_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "ask must block until a decision is submitted"
    );

    let outcome = crate::protocol::permission::submit_active_permission_decision(
        request_id.clone(),
        UiPermissionDecision::AllowOnce,
        rule_identity,
    );
    assert_eq!(
        outcome,
        crate::protocol::permission::SubmitOutcome::Accepted
    );

    let choice = choice_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("resolved choice");
    assert_eq!(choice, AskChoice::AllowOnce);

    let submitted = event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("submitted event");
    match submitted {
        UiEvent::PermissionDecisionSubmitted {
            request_id: submitted_id,
            decision,
            ..
        } => {
            assert_eq!(submitted_id, request_id);
            assert_eq!(decision, UiPermissionDecision::AllowOnce);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    handle.join().expect("ask thread join");
}

#[test]
fn cli_permissions_overlay_rejects_invalid_action_with_explicit_leaf_path() {
    let err = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "read" => Value::test_string("prompt")
    }))
    .expect_err("invalid action must fail");

    assert!(err.contains("permissions.read"));
    assert!(err.contains("invalid permission action"));
}

#[test]
fn additive_overlay_cli_wins_on_overlap_and_retains_non_overlapping() {
    let base = PermissionsConfig::parse_from_plugin_config(
        Some(&Value::test_record(record! {
            "permissions" => Value::test_record(record! {
                "*" => Value::test_string("ask"),
                "read" => Value::test_string("allow"),
                "glob" => Value::test_string("deny")
            })
        })),
        true,
    );

    let overlay = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "read" => Value::test_string("deny"),
        "grep" => Value::test_string("allow")
    }))
    .expect("overlay parse");

    let merged = base.with_overlay(&overlay);

    assert_eq!(
        merged.evaluate("read", &serde_json::json!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        merged.evaluate("glob", &serde_json::json!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        merged.evaluate("grep", &serde_json::json!({})).action,
        PermissionAction::Allow
    );
}

#[test]
fn additive_overlay_merges_nested_nu_run_command_deterministically() {
    let base = PermissionsConfig::parse_from_plugin_config(
        Some(&Value::test_record(record! {
            "permissions" => Value::test_record(record! {
                "*" => Value::test_string("ask"),
                "nu__run" => Value::test_record(record! {
                    "command" => Value::test_record(record! {
                        "kubectl get *" => Value::test_string("allow"),
                        "*" => Value::test_string("ask")
                    })
                })
            })
        })),
        true,
    );

    let overlay = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "nu__run" => Value::test_record(record! {
            "command" => Value::test_record(record! {
                "kubectl delete *" => Value::test_string("deny"),
                "*" => Value::test_string("deny")
            })
        })
    }))
    .expect("overlay parse");

    let merged = base.with_overlay(&overlay);

    assert_eq!(
        merged
            .evaluate(
                "nu__run",
                &serde_json::json!({"command": "kubectl get pods"})
            )
            .action,
        PermissionAction::Allow
    );
    assert_eq!(
        merged
            .evaluate(
                "nu__run",
                &serde_json::json!({"command": "kubectl delete pod x"})
            )
            .action,
        PermissionAction::Deny
    );
    assert_eq!(
        merged
            .evaluate("nu__run", &serde_json::json!({"command": "echo hi"}))
            .action,
        PermissionAction::Deny
    );

    let merged_again = base.with_overlay(&overlay);
    assert_eq!(
        merged_again
            .evaluate("nu__run", &serde_json::json!({"command": "echo hi"}))
            .action,
        PermissionAction::Deny
    );
}

#[test]
#[serial_test::serial]
fn submit_active_permission_decision_without_active_sender_is_ignored() {
    crate::protocol::permission::install_active_permission_submission_sender(None);
    let outcome = crate::protocol::permission::submit_active_permission_decision(
        "unknown".to_string(),
        UiPermissionDecision::Deny,
        "nested:nu__run.command:*".to_string(),
    );
    assert_eq!(
        outcome,
        crate::protocol::permission::SubmitOutcome::Ignored {
            reason: "stale_or_unknown_request"
        }
    );
}

#[test]
fn permission_request_token_rejects_mismatched_rule_identity() {
    let controller = crate::protocol::permission::PermissionController::new(Duration::from_secs(1));
    let request = crate::protocol::permission::PermissionRequest {
        request_id: "ask-0000000000000001".to_string(),
        context: crate::protocol::event::PermissionRequestContext {
            tool: "nu__run".to_string(),
            source: "closure".to_string(),
            mode: None,
            matched_rule_identity: "nested:nu__run.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "summary".to_string(),
            pre_authorize_display: None,
        },
    };
    let (token, _event) = controller.begin_request(request).expect("begin request");

    let outcome = token.submit(PermissionDecisionSubmission {
        request_id: token.request_id().to_string(),
        decision: UiPermissionDecision::AllowOnce,
        matched_rule_identity: "mismatch".to_string(),
    });

    assert_eq!(
        outcome,
        crate::protocol::permission::SubmitOutcome::Ignored {
            reason: "rule_identity_mismatch"
        }
    );
}

#[test]
fn display_tool_name_with_no_args() {
    let result = display_tool_name("skill", &serde_json::json!({}));
    assert_eq!(result, "skill");
}

#[test]
fn display_tool_name_with_single_arg() {
    let result = display_tool_name("skill", &serde_json::json!({"name": "nushell-shell"}));
    assert_eq!(result, "skill(name=nushell-shell)");
}

#[test]
fn display_tool_name_with_multiple_args_sorted_alphabetically() {
    let result = display_tool_name(
        "edit",
        &serde_json::json!({
            "filePath": "src/foo.rs",
            "oldString": "foo",
            "newString": "bar"
        }),
    );
    assert_eq!(
        result,
        "edit(filePath=src/foo.rs, newString=bar, oldString=foo)"
    );
}

#[test]
fn display_tool_name_truncates_long_string_values() {
    let long_value = "a".repeat(70);
    let result = display_tool_name("read", &serde_json::json!({"filePath": long_value}));
    let expected = format!("read(filePath={}…)", "a".repeat(60));
    assert_eq!(result, expected);
}

#[test]
fn display_tool_name_skips_null_values() {
    let result = display_tool_name(
        "edit",
        &serde_json::json!({
            "filePath": "test.rs",
            "mode": null,
            "content": "hello"
        }),
    );
    assert_eq!(result, "edit(content=hello, filePath=test.rs)");
}

#[test]
fn display_tool_name_all_null_args_returns_tool_name_only() {
    let result = display_tool_name(
        "test",
        &serde_json::json!({
            "arg1": null,
            "arg2": null
        }),
    );
    assert_eq!(result, "test");
}

#[test]
fn display_tool_name_shows_nested_object_as_compact_json() {
    let result = display_tool_name(
        "complex",
        &serde_json::json!({
            "config": {"key": "value", "nested": {"deep": true}}
        }),
    );
    assert!(result.starts_with("complex(config="));
    assert!(result.contains(r#"{"key":"value""#));
}

#[test]
fn display_tool_name_truncates_nested_object_json() {
    let long_obj = serde_json::json!({
        "key1": "value1",
        "key2": "value2",
        "key3": "value3",
        "key4": "value4",
        "key5": "value5",
        "key6": "value6"
    });
    let result = display_tool_name("tool", &serde_json::json!({"data": long_obj}));
    assert!(result.starts_with("tool(data="));
    assert!(result.ends_with("…)"));
    // Verify the truncation marker exists
    assert!(result.contains("…"));
}

#[test]
fn display_tool_name_shows_array_as_compact_json() {
    let result = display_tool_name("list", &serde_json::json!({"items": [1, 2, 3]}));
    assert_eq!(result, "list(items=[1,2,3])");
}

#[test]
fn display_tool_name_truncates_long_array() {
    let long_array: Vec<i32> = (0..100).collect();
    let result = display_tool_name("batch", &serde_json::json!({"values": long_array}));
    assert!(result.starts_with("batch(values="));
    assert!(result.ends_with("…)"));
    // Verify the truncation marker exists
    assert!(result.contains("…"));
}

#[test]
fn display_tool_name_shows_boolean_values() {
    let result = display_tool_name(
        "flag",
        &serde_json::json!({"enabled": true, "verbose": false}),
    );
    assert_eq!(result, "flag(enabled=true, verbose=false)");
}

#[test]
fn display_tool_name_shows_number_values() {
    let result = display_tool_name("config", &serde_json::json!({"count": 42, "ratio": 2.72}));
    assert_eq!(result, "config(count=42, ratio=2.72)");
}

#[test]
fn display_tool_name_non_object_args_returns_tool_name_only() {
    assert_eq!(
        display_tool_name("test", &serde_json::json!("string")),
        "test"
    );
    assert_eq!(display_tool_name("test", &serde_json::json!(123)), "test");
    assert_eq!(display_tool_name("test", &serde_json::json!(true)), "test");
    assert_eq!(display_tool_name("test", &serde_json::json!(null)), "test");
    assert_eq!(display_tool_name("test", &serde_json::json!([])), "test");
}

#[test]
fn parse_from_yaml_global_allow() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        "*": "allow"
        "#,
    )
    .expect("valid YAML");

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, Some(PermissionAction::Allow));
    assert!(overlay.tool_rules.is_empty());
    assert!(overlay.nu_run_command_rules.is_empty());
}

#[test]
fn parse_from_yaml_tool_rules() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        read: "allow"
        nu__run: "deny"
        "#,
    )
    .expect("valid YAML");

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, None);
    assert_eq!(overlay.tool_rules.len(), 2);
    assert_eq!(
        overlay.tool_rules.get("read"),
        Some(&PermissionAction::Allow)
    );
    assert_eq!(
        overlay.tool_rules.get("nu__run"),
        Some(&PermissionAction::Deny)
    );
    assert!(overlay.nu_run_command_rules.is_empty());
}

#[test]
fn parse_from_yaml_nu_run_commands() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        nu__run:
          command:
            "cargo*": "allow"
            "rm*": "deny"
        "#,
    )
    .expect("valid YAML");

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, None);
    assert!(overlay.tool_rules.is_empty());
    assert_eq!(overlay.nu_run_command_rules.len(), 2);
    assert_eq!(
        overlay.nu_run_command_rules.get("cargo*"),
        Some(&PermissionAction::Allow)
    );
    assert_eq!(
        overlay.nu_run_command_rules.get("rm*"),
        Some(&PermissionAction::Deny)
    );
}

#[test]
fn parse_from_yaml_invalid_action() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        "*": "yolo"
        "#,
    )
    .expect("valid YAML");

    let err = PermissionsOverlay::parse_from_yaml(&yaml).expect_err("invalid action should fail");
    assert!(err.contains("permissions.*"));
    assert!(err.contains("invalid permission action"));
}

#[test]
fn parse_from_yaml_invalid_structure() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        nu__run: 42
        "#,
    )
    .expect("valid YAML");

    let err =
        PermissionsOverlay::parse_from_yaml(&yaml).expect_err("invalid structure should fail");
    assert!(err.contains("permissions.nu__run"));
}

#[test]
fn parse_from_yaml_empty_mapping() {
    let yaml = noyalib::Mapping::new();

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, None);
    assert!(overlay.tool_rules.is_empty());
    assert!(overlay.nu_run_command_rules.is_empty());
}

#[test]
fn parse_from_yaml_mixed() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        "*": "ask"
        read: "allow"
        write: "deny"
        nu__run:
          command:
            "kubectl*": "deny"
            "*": "ask"
        "#,
    )
    .expect("valid YAML");

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, Some(PermissionAction::Ask));
    assert_eq!(overlay.tool_rules.len(), 2);
    assert_eq!(
        overlay.tool_rules.get("read"),
        Some(&PermissionAction::Allow)
    );
    assert_eq!(
        overlay.tool_rules.get("write"),
        Some(&PermissionAction::Deny)
    );
    assert_eq!(overlay.nu_run_command_rules.len(), 2);
    assert_eq!(
        overlay.nu_run_command_rules.get("kubectl*"),
        Some(&PermissionAction::Deny)
    );
    assert_eq!(
        overlay.nu_run_command_rules.get("*"),
        Some(&PermissionAction::Ask)
    );
}

// ── is_tool_visible tests ──────────────────────────────────────────────

#[test]
fn is_tool_visible_returns_true_for_global_ask() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
        })
    });
    let config = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    assert!(config.is_tool_visible("nu__run"));
    assert!(config.is_tool_visible("read"));
}

#[test]
fn is_tool_visible_returns_true_for_global_allow() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("allow"),
        })
    });
    let config = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    assert!(config.is_tool_visible("nu__run"));
}

#[test]
fn is_tool_visible_returns_false_for_global_deny() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("deny"),
        })
    });
    let config = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    assert!(!config.is_tool_visible("nu__run"));
    assert!(!config.is_tool_visible("read"));
}

#[test]
fn is_tool_visible_returns_false_for_tool_level_deny() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "nu__run" => Value::test_string("deny"),
        })
    });
    let config = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    assert!(!config.is_tool_visible("nu__run"));
    assert!(config.is_tool_visible("read"));
}

#[test]
fn is_tool_visible_returns_true_for_granular_deny_with_tool_level_ask() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "nu__run" => Value::test_record(record! {
                "command" => Value::test_record(record! {
                    "kubectl delete *" => Value::test_string("deny"),
                    "*" => Value::test_string("ask"),
                })
            })
        })
    });
    let config = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    assert!(config.is_tool_visible("nu__run"));
}

#[test]
fn is_tool_visible_respects_specificity_over_global() {
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("deny"),
            "read" => Value::test_string("allow"),
        })
    });
    let config = PermissionsConfig::parse_from_plugin_config(Some(&value), true);
    assert!(config.is_tool_visible("read"));
    assert!(!config.is_tool_visible("nu__run"));
    assert!(!config.is_tool_visible("edit"));
}

// ── safe_defaults(interactive) tests ───────────────────────────────────

#[test]
fn safe_defaults_tui_mode_global_ask() {
    let config = PermissionsConfig::safe_defaults(true);
    let decision = config.evaluate("unknown_tool", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Ask);
}

#[test]
fn safe_defaults_tty_mode_global_deny() {
    let config = PermissionsConfig::safe_defaults(false);
    let decision = config.evaluate("unknown_tool", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Deny);
}

#[test]
fn safe_defaults_tty_mode_still_allows_read_tools() {
    let config = PermissionsConfig::safe_defaults(false);
    assert!(config.is_tool_visible("read"));
    assert!(config.is_tool_visible("glob"));
    assert!(config.is_tool_visible("grep"));
}

#[test]
fn safe_defaults_tty_mode_hides_unknown_tools() {
    let config = PermissionsConfig::safe_defaults(false);
    assert!(!config.is_tool_visible("nu__run"));
    assert!(!config.is_tool_visible("nu__shell"));
    assert!(!config.is_tool_visible("edit"));
}
