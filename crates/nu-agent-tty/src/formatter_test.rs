use crate::formatter::{ToolEndView, format_tool_start};
use crate::policy::Verbosity;

#[test]
fn default_level_shows_tool_name_and_status_only() {
    let start = format_tool_start(Verbosity::Normal, "gh__list_prs", "mcp", "{\"q\":\"x\"}");
    let end = ToolEndView {
        verbosity: Verbosity::Normal,
        name: "gh__list_prs",
        source: "mcp",
        arguments: "{\"q\":\"x\"}",
        success: true,
        result: "[]",
        error_kind: None,
        message: None,
    }
    .format();

    assert_eq!(start, "tool gh__list_prs");
    assert!(!start.contains("args:"));
    assert_eq!(end, "✓ tool gh__list_prs → {\"q\":\"x\"}\n[]");
}

#[test]
fn v_level_includes_concise_source_args_and_result() {
    let start = format_tool_start(Verbosity::Verbose, "gh__list_prs", "mcp", "{\"q\":\"x\"}");
    let end = ToolEndView {
        verbosity: Verbosity::Verbose,
        name: "gh__list_prs",
        source: "mcp",
        arguments: "{\"q\":\"x\"}",
        success: true,
        result: "[]",
        error_kind: None,
        message: None,
    }
    .format();

    assert!(start.contains("(mcp)"));
    assert!(start.contains("→ "));
    assert!(end.contains("\n[]"));
    assert!(end.starts_with("✓ tool gh__list_prs (mcp)"));
    assert!(end.contains("→ {\"q\":\"x\"}"));
}

#[test]
fn vv_and_vvv_use_multiline_with_truncation_guards() {
    let huge = "x".repeat(20_000);
    let vv = ToolEndView {
        verbosity: Verbosity::VeryVerbose,
        name: "tool",
        source: "closure",
        arguments: "{\"a\":1}",
        success: true,
        result: &huge,
        error_kind: None,
        message: None,
    }
    .format();
    let vvv = ToolEndView {
        verbosity: Verbosity::Trace,
        name: "tool",
        source: "closure",
        arguments: "{\"a\":1}",
        success: true,
        result: &huge,
        error_kind: None,
        message: None,
    }
    .format();

    assert!(vv.contains("\n"));
    assert!(vv.ends_with('…'));
    assert!(vv.chars().count() < vvv.chars().count());
    assert!(vvv.chars().count() < huge.chars().count());
}

#[test]
fn default_level_uses_newline_separated_result_block() {
    let end = ToolEndView {
        verbosity: Verbosity::Normal,
        name: "k8s__list_pods",
        source: "mcp",
        arguments: "{}",
        success: false,
        result: "{\"error\":\"denied\"}",
        error_kind: Some("permission"),
        message: Some("rbac denied"),
    }
    .format();

    let lines: Vec<_> = end.lines().collect();
    assert_eq!(lines[0], "✗ tool k8s__list_pods → {}");
    assert_eq!(lines[1], "{\"error\":\"denied\"}");
}

#[test]
fn default_level_shows_non_empty_json_payloads() {
    let empty_arr = ToolEndView {
        verbosity: Verbosity::Normal,
        name: "gh__list_prs",
        source: "mcp",
        arguments: "{}",
        success: true,
        result: "[]",
        error_kind: None,
        message: None,
    }
    .format();
    assert_eq!(empty_arr, "✓ tool gh__list_prs → {}\n[]");

    let empty_obj = ToolEndView {
        verbosity: Verbosity::Normal,
        name: "gh__get_pr",
        source: "mcp",
        arguments: "{}",
        success: true,
        result: "{}",
        error_kind: None,
        message: None,
    }
    .format();
    assert_eq!(empty_obj, "✓ tool gh__get_pr → {}\n{}");
}

#[test]
fn default_level_truncates_long_result_output() {
    let long_result = "x".repeat(500);
    let end = ToolEndView {
        verbosity: Verbosity::Normal,
        name: "gh__run_workflow",
        source: "mcp",
        arguments: "{}",
        success: true,
        result: &long_result,
        error_kind: None,
        message: None,
    }
    .format();

    let lines: Vec<&str> = end.lines().collect();
    assert_eq!(lines[0], "✓ tool gh__run_workflow → {}");
    assert!(lines[1].ends_with('…'));
    assert!(lines[1].chars().count() <= 121);
}

#[test]
fn v_level_shows_full_result_output() {
    let long_result = "x".repeat(500);
    let end = ToolEndView {
        verbosity: Verbosity::Verbose,
        name: "gh__run_workflow",
        source: "mcp",
        arguments: "{}",
        success: true,
        result: &long_result,
        error_kind: None,
        message: None,
    }
    .format();

    assert!(end.contains("\n"));
    assert!(end.contains(&long_result));
    assert!(end.contains("→ {}"));
}
