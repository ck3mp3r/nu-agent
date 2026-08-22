// === Imports ===
use std::fs;
use std::time::Duration;

use tempfile::tempdir;

use super::agents::load_agents_chain;
use super::compaction::{
    CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
    TokenCompactionPolicy,
};
use super::skills::{
    SkillResolveError, SkillSource, discover_skill_catalog, extract_skill_description,
    is_higher_precedence, render_available_skills_preamble_from_catalog,
    resolve_explicit_skill_request,
};
use super::slash::{
    SLASH_COMMAND_ORDER, SlashCommand, SlashParseResult, extract_session_id,
    filter_inline_slash_suggestions, parse_slash_command,
};

use crate::compaction::CompactionStrategy;
use crate::protocol::event::{
    PermissionDecision, PermissionDecisionSubmission, PermissionRequestContext, UiEvent,
};
use crate::protocol::permission::{
    PermissionController, PermissionRequest, PermissionResolution, RequestError, SubmitOutcome,
};

// === Tests: agents ===

#[test]
fn loads_home_agents_when_present() {
    let tmp = tempdir().expect("tempdir");
    let config = tmp.path().join("config");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(config.join("agents")).expect("config/agents");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(config.join("agents/AGENTS.md"), "CONFIG\n").expect("write config agents");

    let loaded = load_agents_chain(&cwd, Some(&config), Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("CONFIG\n"));
    assert!(loaded.warnings.is_empty());
}

#[test]
fn loads_cwd_agents_when_present() {
    let tmp = tempdir().expect("tempdir");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(cwd.join("AGENTS.md"), "CWD\n").expect("write cwd agents");

    let loaded = load_agents_chain(&cwd, None, Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("CWD\n"));
    assert!(loaded.warnings.is_empty());
}

#[test]
fn loads_ancestor_agents_in_root_to_leaf_order() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let parent = root.join("a");
    let cwd = parent.join("b");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(root.join("AGENTS.md"), "ROOT\n").expect("root agents");
    fs::write(parent.join("AGENTS.md"), "PARENT\n").expect("parent agents");

    let loaded = load_agents_chain(&cwd, None, Some(&root), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("ROOT\n\nPARENT\n"));
    assert!(loaded.warnings.is_empty());
}

#[test]
fn nearest_agents_has_highest_precedence_position() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let cwd = root.join("child");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(root.join("AGENTS.md"), "ROOT\n").expect("root agents");
    fs::write(cwd.join("AGENTS.md"), "CWD\n").expect("cwd agents");

    let loaded = load_agents_chain(&cwd, None, Some(&root), None);
    let merged = loaded.merged_chain.expect("merged chain");

    assert!(merged.ends_with("CWD\n"));
    assert!(merged.contains("ROOT\n"));
}

#[test]
fn missing_home_or_agents_files_is_noop() {
    let tmp = tempdir().expect("tempdir");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(&cwd).expect("cwd");

    let loaded = load_agents_chain(&cwd, None, Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain, None);
    assert!(loaded.warnings.is_empty());
}

#[test]
fn unreadable_agents_is_non_fatal() {
    let tmp = tempdir().expect("tempdir");
    let cwd = tmp.path().join("cwd");
    let agents = cwd.join("AGENTS.md");
    fs::create_dir_all(&agents).expect("create AGENTS.md directory to force read error");

    let loaded = load_agents_chain(&cwd, None, Some(tmp.path()), None);

    assert!(loaded.warnings.iter().any(|w| w.contains("AGENTS.md")));
    assert_eq!(loaded.merged_chain, None);
}

#[test]
fn canonical_path_dedup_prevents_duplicate_load() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let real = root.join("real");
    let alias = root.join("alias");
    fs::create_dir_all(&real).expect("real");
    fs::write(real.join("AGENTS.md"), "REAL\n").expect("write agents");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &alias).expect("symlink alias->real");

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &alias).expect("symlink alias->real");

    let cwd = alias.join("child");
    fs::create_dir_all(&cwd).expect("cwd");

    let loaded = load_agents_chain(&cwd, None, Some(&root), None);
    let merged = loaded.merged_chain.expect("merged chain");

    assert_eq!(merged.matches("REAL").count(), 1);
}

#[test]
fn loads_from_home_dot_agents() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(home.join(".agents")).expect("create .agents");
    fs::create_dir_all(&cwd).expect("create cwd");
    fs::write(home.join(".agents/AGENTS.md"), "HOME_AGENTS\n").expect("write");

    let result = load_agents_chain(&cwd, None, Some(tmp.path()), Some(&home));
    assert!(result.merged_chain.unwrap().contains("HOME_AGENTS"));
}

// === Tests: compaction ===

#[test]
fn fires_at_threshold_percentage() {
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(160_000));
    assert!(matches!(decision, CompactionTriggerDecision::Fire { .. }));
}

#[test]
fn does_not_fire_below_threshold() {
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(159_999));
    assert!(matches!(decision, CompactionTriggerDecision::NoFire { .. }));
}

#[test]
fn does_not_fire_when_no_token_data() {
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(None);
    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "no_token_data".to_string()
        }
    );
}

#[test]
fn does_not_fire_with_zero_context_window() {
    let policy = TokenCompactionPolicy::new(0, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(100_000));
    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "zero_context_window".to_string()
        }
    );
}

#[test]
fn does_not_fire_with_zero_tokens() {
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(0));
    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "zero_tokens".to_string()
        }
    );
}

#[test]
fn fires_at_threshold_with_nonzero_tokens() {
    // Regression: Some(1) with a large context window stays below threshold.
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(1));
    assert!(matches!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: ref r
        } if r.contains("below_threshold")
    ));
}

#[test]
fn respects_custom_threshold_percentage() {
    let policy = TokenCompactionPolicy::new(128_000, 0.90, CompactionStrategy::SlidingSummary);
    // 90% of 128k = 115,200
    let below = policy.evaluate(Some(115_199));
    let at = policy.evaluate(Some(115_200));
    assert!(matches!(below, CompactionTriggerDecision::NoFire { .. }));
    assert!(matches!(at, CompactionTriggerDecision::Fire { .. }));
}

#[test]
fn fire_includes_configured_strategy() {
    let policy = TokenCompactionPolicy::new(100_000, 0.80, CompactionStrategy::SlidingWindow);
    let decision = policy.evaluate(Some(80_000));
    assert_eq!(
        decision,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "token_usage_80pct_of_80pct_threshold".to_string(),
            strategy: CompactionStrategy::SlidingWindow,
        }
    );
}

#[test]
fn no_state_needed_between_evaluations() {
    // Token-based policy is stateless — calling evaluate twice with same input gives same result
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let first = policy.evaluate(Some(160_000));
    let second = policy.evaluate(Some(160_000));
    assert_eq!(first, second);
}

// === Tests: event_contract ===

#[test]
fn ui_event_contract_exposes_required_variants() {
    let events = [
        UiEvent::LlmStart,
        UiEvent::Tick,
        UiEvent::LlmEnd {
            response_chars: 12,
            tool_calls: 1,
            input_tokens: 7,
            output_tokens: 5,
            total_tokens: 12,
        },
        UiEvent::ToolStart {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        },
        UiEvent::ToolEnd {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
        UiEvent::PermissionRequested {
            request_id: "ask-0000000000000001".to_string(),
            context: PermissionRequestContext {
                tool: "nu".to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "nested:nu.command:*".to_string(),
                scope: "nested".to_string(),
                target_field: Some("command".to_string()),
                pattern: "*".to_string(),
                summary: "→ {\"command\":\"echo hi\"}".to_string(),
                pre_authorize_display: None,
            },
        },
        UiEvent::PermissionDecisionSubmitted {
            request_id: "ask-0000000000000001".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "nested:nu.command:*".to_string(),
        },
        UiEvent::PermissionDecisionTimedOut {
            request_id: "ask-0000000000000002".to_string(),
        },
        UiEvent::PermissionDecisionIgnored {
            request_id: "ask-0000000000000003".to_string(),
            reason: "stale_or_unknown_request".to_string(),
        },
        UiEvent::Warning {
            message: "compaction failed".to_string(),
        },
        UiEvent::CompactionStarted {
            source: "auto_threshold".to_string(),
        },
        UiEvent::CompactionSummaryChunk {
            source: "auto_threshold".to_string(),
            delta: "chunk".to_string(),
            aggregated: "chunk".to_string(),
        },
        UiEvent::CompactionTriggered {
            source: "auto_threshold".to_string(),
            summarized_count: 3,
            kept_recent_count: 2,
            summary_preview: "summary preview".to_string(),
            summary_body: "summary body".to_string(),
        },
        UiEvent::CompactionFailed {
            source: "auto_threshold".to_string(),
            message: "failed".to_string(),
        },
        UiEvent::AssistantMessage {
            text: "done".to_string(),
        },
        UiEvent::Completed { tool_calls: 1 },
    ];

    assert_eq!(events.len(), 16);
}

#[test]
fn permission_event_field_shape_is_explicit_and_stable() {
    let requested = UiEvent::PermissionRequested {
        request_id: "ask-0000000000000001".to_string(),
        context: PermissionRequestContext {
            tool: "nu(command=echo hi)".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        },
    };
    match requested {
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => {
            assert_eq!(request_id, "ask-0000000000000001");
            assert_eq!(context.tool, "nu(command=echo hi)");
            assert_eq!(context.source, "closure");
            assert_eq!(context.mode.as_deref(), Some("apply"));
            assert_eq!(context.matched_rule_identity, "nested:nu.command:*");
            assert_eq!(context.scope, "nested");
            assert_eq!(context.target_field.as_deref(), Some("command"));
            assert_eq!(context.pattern, "*");
            assert!(context.summary.starts_with("→ "));
            assert!(context.pre_authorize_display.is_none());
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let submitted = UiEvent::PermissionDecisionSubmitted {
        request_id: "ask-0000000000000001".to_string(),
        decision: PermissionDecision::AllowAlways,
        matched_rule_identity: "nested:nu.command:*".to_string(),
    };
    match submitted {
        UiEvent::PermissionDecisionSubmitted {
            request_id,
            decision,
            matched_rule_identity,
        } => {
            assert_eq!(request_id, "ask-0000000000000001");
            assert_eq!(decision.as_str(), "allow_always");
            assert_eq!(matched_rule_identity, "nested:nu.command:*");
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let timed_out = UiEvent::PermissionDecisionTimedOut {
        request_id: "ask-0000000000000002".to_string(),
    };
    match timed_out {
        UiEvent::PermissionDecisionTimedOut { request_id } => {
            assert_eq!(request_id, "ask-0000000000000002");
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let ignored = UiEvent::PermissionDecisionIgnored {
        request_id: "ask-0000000000000003".to_string(),
        reason: "rule_identity_mismatch".to_string(),
    };
    match ignored {
        UiEvent::PermissionDecisionIgnored { request_id, reason } => {
            assert_eq!(request_id, "ask-0000000000000003");
            assert_eq!(reason, "rule_identity_mismatch");
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}

// === Tests: permission ===

fn request_with_id(request_id: &str) -> PermissionRequest {
    PermissionRequest {
        request_id: request_id.to_string(),
        context: PermissionRequestContext {
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        },
    }
}

#[test]
fn begin_request_rejects_duplicate_request_id() {
    let controller = PermissionController::new(Duration::from_millis(100));
    let first = controller.begin_request(request_with_id("ask-0000000000000001"));
    assert!(first.is_ok());

    let second = controller.begin_request(request_with_id("ask-0000000000000001"));
    assert!(matches!(second, Err(RequestError::AlreadyWaiting)));
}

#[tokio::test]
async fn await_resolution_emits_ignored_event_for_rule_identity_mismatch_then_times_out() {
    let controller = PermissionController::new(Duration::from_millis(30));
    let (token, _event) = controller
        .begin_request(request_with_id("ask-0000000000000002"))
        .expect("begin request");

    let outcome = token.submit(PermissionDecisionSubmission {
        request_id: token.request_id().to_string(),
        decision: PermissionDecision::AllowAlways,
        matched_rule_identity: "wrong-rule".to_string(),
    });
    assert_eq!(
        outcome,
        SubmitOutcome::Ignored {
            reason: "rule_identity_mismatch"
        }
    );

    let (resolution, events) = controller.await_resolution(&token).await;
    assert_eq!(resolution, PermissionResolution::TimedOut);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionDecisionTimedOut { .. }))
    );
}

#[tokio::test]
async fn await_resolution_ignores_stale_submission_and_accepts_matching_submission() {
    let controller = PermissionController::new(Duration::from_secs(1));
    let (token, _event) = controller
        .begin_request(request_with_id("ask-0000000000000003"))
        .expect("begin request");

    let sender = token.sender_clone();
    sender
        .send(PermissionDecisionSubmission {
            request_id: "stale-request-id".to_string(),
            decision: PermissionDecision::AllowAlways,
            matched_rule_identity: token.matched_rule_identity().to_string(),
        })
        .expect("send stale submission");
    sender
        .send(PermissionDecisionSubmission {
            request_id: token.request_id().to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: token.matched_rule_identity().to_string(),
        })
        .expect("send matching submission");

    let (resolution, events) = controller.await_resolution(&token).await;
    assert_eq!(
        resolution,
        PermissionResolution::Decision {
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: token.matched_rule_identity().to_string(),
        }
    );
    assert!(events.iter().any(|event| matches!(
        event,
        UiEvent::PermissionDecisionIgnored {
            request_id,
            reason
        } if request_id == "stale-request-id" && reason == "stale_or_unknown_request"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        UiEvent::PermissionDecisionSubmitted { request_id, decision, .. }
            if request_id == token.request_id() && *decision == PermissionDecision::AllowOnce
    )));
}

// === Tests: skills ===

#[test]
fn discovers_home_skills_in_catalog() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/nushell-shell")).expect("home skill dir");
    fs::write(
        home.join(".agents/skills/nushell-shell/SKILL.md"),
        "home skill content\n",
    )
    .expect("home skill");
    fs::create_dir_all(&cwd).expect("cwd");

    let catalog = discover_skill_catalog(&cwd, Some(&home), Some(tmp.path()));

    assert!(
        catalog
            .iter()
            .any(|entry| { entry.name == "nushell-shell" && entry.source == SkillSource::Home })
    );
}

#[test]
fn resolves_explicit_skill_request_from_home_source() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/context")).expect("home skill dir");
    fs::write(
        home.join(".agents/skills/context/SKILL.md"),
        "home context skill\n",
    )
    .expect("home skill");
    fs::create_dir_all(&cwd).expect("cwd");

    let resolved = resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "context")
        .expect("resolve should succeed")
        .expect("skill must resolve");

    assert_eq!(resolved.source, SkillSource::Home);
    assert_eq!(resolved.content, "home context skill\n");
}

#[test]
fn local_source_wins_on_skill_name_collision() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/context")).expect("home skill dir");
    fs::write(home.join(".agents/skills/context/SKILL.md"), "home\n").expect("home skill");

    fs::create_dir_all(repo.join(".agents/skills/context")).expect("local skill dir");
    fs::write(repo.join(".agents/skills/context/SKILL.md"), "local\n").expect("local skill");

    let resolved = resolve_explicit_skill_request(&repo, Some(&home), Some(tmp.path()), "context")
        .expect("resolve should succeed")
        .expect("skill must resolve");

    assert_eq!(resolved.source, SkillSource::Local);
    assert_eq!(resolved.content, "local\n");
}

#[test]
fn rejects_path_traversal_skill_lookup() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    fs::create_dir_all(home.join(".agents/skills/context")).expect("home skill dir");
    fs::write(home.join(".agents/skills/context/SKILL.md"), "home\n").expect("home skill");
    fs::create_dir_all(&cwd).expect("cwd");

    let err = resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "../context")
        .expect_err("traversal must be rejected");

    assert!(matches!(err, SkillResolveError::InvalidSkillName(_)));
}

#[test]
fn rejects_symlink_escape_outside_home_skills_root() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    let outside = tmp.path().join("outside");

    fs::create_dir_all(home.join(".agents/skills")).expect("home skills root");
    fs::create_dir_all(&outside).expect("outside");
    fs::create_dir_all(&cwd).expect("cwd");

    fs::write(outside.join("SKILL.md"), "outside\n").expect("outside skill");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, home.join(".agents/skills/escaped")).expect("symlink");

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, home.join(".agents/skills/escaped"))
        .expect("symlink");

    let err = resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "escaped")
        .expect_err("symlink escape must be rejected");

    assert!(matches!(
        err,
        SkillResolveError::HomeSkillEscapesRoot { skill_name } if skill_name == "escaped"
    ));
}

#[test]
fn missing_skill_preserves_not_found_semantics() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    fs::create_dir_all(home.join(".agents/skills")).expect("home skills root");
    fs::create_dir_all(&cwd).expect("cwd");

    let resolved =
        resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "does-not-exist")
            .expect("missing skill should not error");

    assert_eq!(resolved, None);
}

#[test]
fn precedence_ordering_handles_deep_ancestry_rank_without_packing_collision() {
    let local_deep = (SkillSource::Local.priority(), 16);
    let home = (SkillSource::Home.priority(), 0);

    assert!(
        is_higher_precedence(local_deep, home),
        "local source precedence must remain stable even when ancestry rank >= 16"
    );
    assert!(
        !is_higher_precedence(home, local_deep),
        "distinct precedence tuples must not collapse into an equal rank"
    );
}

#[test]
fn available_skills_preamble_renders_catalog_entries() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/context")).expect("local skills dir");
    fs::write(repo.join(".agents/skills/context/SKILL.md"), "context\n").expect("skill file");

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .expect("preamble should render");

    assert!(preamble.contains("<available_skills>"));
    assert!(preamble.contains("<name>context</name>"));
    assert!(preamble.contains("<source>local</source>"));
}

#[test]
fn available_skills_preamble_absent_when_catalog_empty() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ));
    assert!(preamble.is_none());
}

#[test]
fn skill_preamble_xml_structure_with_description() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/example")).expect("local skills dir");
    fs::write(
        repo.join(".agents/skills/example/SKILL.md"),
        "# Example Skill\n\nThis is an example description.\n",
    )
    .expect("skill file");

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .expect("preamble should render");

    // Verify the XML structure includes description between name and source
    let expected_structure = r#"  <skill>
    <name>example</name>
    <description>This is an example description.</description>
    <source>local</source>
  </skill>"#;

    assert!(preamble.contains(expected_structure));
}

#[test]
fn extract_skill_description_from_first_non_heading_line() {
    let content = r#"# Skill: nushell-shell

# Nushell Shell Patterns

This skill covers using Nushell as a shell, including redirection.

More content here.
"#;
    let desc = extract_skill_description(content).expect("should extract description");
    assert_eq!(
        desc,
        "This skill covers using Nushell as a shell, including redirection."
    );
}

#[test]
fn extract_skill_description_truncates_long_lines() {
    let long_line = "a".repeat(200);
    let content = format!("# Heading\n\n{long_line}\n");
    let desc = extract_skill_description(&content).expect("should extract description");
    assert_eq!(desc.len(), 153); // 150 + '…' (3 bytes in UTF-8)
    assert!(desc.ends_with('…'));
}

#[test]
fn extract_skill_description_returns_none_when_only_headings() {
    let content = "# Heading 1\n## Heading 2\n### Heading 3\n";
    let desc = extract_skill_description(content);
    assert!(desc.is_none());
}

#[test]
fn extract_skill_description_skips_empty_lines() {
    let content = "# Heading\n\n\n\nActual description here.\n";
    let desc = extract_skill_description(content).expect("should extract description");
    assert_eq!(desc, "Actual description here.");
}

#[test]
fn available_skills_preamble_includes_descriptions() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/context")).expect("local skills dir");
    fs::write(
        repo.join(".agents/skills/context/SKILL.md"),
        "# Context Skill\n\nManage context effectively.\n",
    )
    .expect("skill file");

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .expect("preamble should render");

    assert!(preamble.contains("<available_skills>"));
    assert!(preamble.contains("<name>context</name>"));
    assert!(preamble.contains("<description>Manage context effectively.</description>"));
    assert!(preamble.contains("<source>local</source>"));
}

#[test]
fn available_skills_preamble_works_without_descriptions() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/empty")).expect("local skills dir");
    fs::write(
        repo.join(".agents/skills/empty/SKILL.md"),
        "# Only heading\n",
    )
    .expect("skill file");

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .expect("preamble should render");

    assert!(preamble.contains("<name>empty</name>"));
    assert!(!preamble.contains("<description>"));
}

#[test]
fn extract_skill_description_from_frontmatter() {
    let content = "---\nname: context\ndescription: Working effectively with c5t.\nlicense: GPL-2.0\n---\n\n# c5t Context Management\n\nc5t is a personal context manager.\n";
    let desc = extract_skill_description(content).expect("should extract");
    assert_eq!(desc, "Working effectively with c5t.");
}

#[test]
fn extract_skill_description_falls_back_to_body_when_no_frontmatter_description() {
    let content =
        "---\nname: nushell\nlicense: GPL-2.0\n---\n\n# Nushell Guide\n\nBody description here.\n";
    let desc = extract_skill_description(content).expect("should extract");
    assert_eq!(desc, "Body description here.");
}

#[test]
fn extract_skill_description_handles_quoted_frontmatter_value() {
    let content = "---\ndescription: \"Use Nushell as a shell.\"\n---\n\n# Heading\n";
    let desc = extract_skill_description(content).expect("should extract");
    assert_eq!(desc, "Use Nushell as a shell.");
}

// === Tests: slash ===

#[test]
fn parse_slash_command_compact_mcp_help_status_exact() {
    assert_eq!(
        parse_slash_command("   /compact   "),
        SlashParseResult::Command(SlashCommand::Compact)
    );
    assert_eq!(
        parse_slash_command(" /mcp "),
        SlashParseResult::Command(SlashCommand::Mcp)
    );
    assert_eq!(
        parse_slash_command(" /help "),
        SlashParseResult::Command(SlashCommand::Help)
    );
    assert_eq!(
        parse_slash_command(" /status "),
        SlashParseResult::Command(SlashCommand::Status)
    );
}

#[test]
fn parse_slash_command_models_exact() {
    assert_eq!(
        parse_slash_command(" /models "),
        SlashParseResult::Command(SlashCommand::Models)
    );
}

#[test]
fn parse_slash_command_non_slash_returns_not_slash() {
    assert_eq!(
        parse_slash_command("hello world"),
        SlashParseResult::NotSlash
    );
}

#[test]
fn parse_slash_command_unknown_returns_unknown() {
    assert_eq!(
        parse_slash_command("/compact now"),
        SlashParseResult::Unknown("/compact now".to_string())
    );
}

#[test]
fn inline_slash_filter_is_prefix_based_and_deterministic() {
    assert_eq!(
        filter_inline_slash_suggestions("/"),
        vec![
            SlashCommand::Compact,
            SlashCommand::Mcp,
            SlashCommand::Help,
            SlashCommand::Status,
            SlashCommand::Models,
            SlashCommand::Agent,
            SlashCommand::New,
            SlashCommand::Session,
            SlashCommand::Theme,
            SlashCommand::Skills,
        ]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/c"),
        vec![SlashCommand::Compact]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/co"),
        vec![SlashCommand::Compact]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/m"),
        vec![SlashCommand::Mcp, SlashCommand::Models]
    );
    assert!(filter_inline_slash_suggestions("/x").is_empty());
    assert!(filter_inline_slash_suggestions("hello").is_empty());
}

#[test]
fn slash_command_catalog_exports_expected_labels_and_order() {
    assert_eq!(
        SLASH_COMMAND_ORDER,
        [
            SlashCommand::Compact,
            SlashCommand::Mcp,
            SlashCommand::Help,
            SlashCommand::Status,
            SlashCommand::Models,
            SlashCommand::Agent,
            SlashCommand::New,
            SlashCommand::Session,
            SlashCommand::Theme,
            SlashCommand::Skills,
        ]
    );

    assert_eq!(SlashCommand::Compact.label(), "/compact");
    assert_eq!(SlashCommand::Mcp.label(), "/mcp");
    assert_eq!(SlashCommand::Help.label(), "/help");
    assert_eq!(SlashCommand::Status.label(), "/status");
    assert_eq!(SlashCommand::Models.label(), "/models");
    assert_eq!(SlashCommand::Agent.label(), "/agent");
    assert_eq!(SlashCommand::New.label(), "/new");
    assert_eq!(SlashCommand::Session.label(), "/session");
    assert_eq!(SlashCommand::Theme.label(), "/theme");
    assert_eq!(SlashCommand::Skills.label(), "/skills");

    assert!(!SlashCommand::Compact.summary().is_empty());
    assert!(!SlashCommand::Mcp.summary().is_empty());
    assert!(!SlashCommand::Help.summary().is_empty());
    assert!(!SlashCommand::Status.summary().is_empty());
    assert!(!SlashCommand::Models.summary().is_empty());
    assert!(!SlashCommand::Agent.summary().is_empty());
    assert!(!SlashCommand::New.summary().is_empty());
    assert!(!SlashCommand::Session.summary().is_empty());
    assert!(!SlashCommand::Theme.summary().is_empty());
    assert!(!SlashCommand::Skills.summary().is_empty());
}

#[test]
fn parse_slash_command_agent_exact() {
    assert_eq!(
        parse_slash_command("/agent"),
        SlashParseResult::Command(SlashCommand::Agent)
    );
}

#[test]
fn parse_slash_command_agents_does_not_match() {
    assert_eq!(
        parse_slash_command("/agents"),
        SlashParseResult::Unknown("/agents".to_string())
    );
}

#[test]
fn slash_command_label_agent_returns_slash_agent() {
    assert_eq!(SlashCommand::Agent.label(), "/agent");
}

#[test]
fn slash_command_summary_agent_returns_switch_agent_persona() {
    assert_eq!(SlashCommand::Agent.summary(), "Switch agent persona");
}

#[test]
fn parse_slash_command_session_exact() {
    assert_eq!(
        parse_slash_command("/session"),
        SlashParseResult::Command(SlashCommand::Session)
    );
}

#[test]
fn parse_slash_command_session_case_insensitive() {
    assert_eq!(
        parse_slash_command("/SESSION"),
        SlashParseResult::Command(SlashCommand::Session)
    );
}

#[test]
fn parse_slash_command_session_with_id() {
    assert_eq!(
        parse_slash_command("/session abc123"),
        SlashParseResult::Command(SlashCommand::Session)
    );
}

#[test]
fn extract_session_id_returns_none_for_bare_session() {
    assert_eq!(extract_session_id("/session"), None);
}

#[test]
fn extract_session_id_returns_id_argument() {
    assert_eq!(extract_session_id("/session abc123"), Some("abc123"));
}

#[test]
fn extract_session_id_case_insensitive_prefix() {
    assert_eq!(extract_session_id("/SESSION abc123"), Some("abc123"));
    assert_eq!(extract_session_id("/Session abc123"), Some("abc123"));
}

#[test]
fn extract_session_id_trims_whitespace() {
    assert_eq!(extract_session_id("  /session   abc123  "), Some("abc123"));
}
