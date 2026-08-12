use super::{
    pane_target, parse_panes, parse_panes_find, parse_sessions, parse_size, parse_windows,
    require_force, resolve_dir,
};
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[test]
fn parse_sessions_parses_pipe_delimited_lines() {
    let output = "main|3|1700000000|1\nscratch|1|1700000100|0\n";
    let sessions = parse_sessions(output);

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0]["name"], "main");
    assert_eq!(sessions[0]["windows"], 3);
    assert_eq!(sessions[0]["created"], 1700000000);
    assert_eq!(sessions[0]["attached"], 1);
    assert_eq!(sessions[1]["name"], "scratch");
    assert_eq!(sessions[1]["windows"], 1);
    assert_eq!(sessions[1]["attached"], 0);
}

#[test]
fn parse_sessions_handles_empty_output() {
    let sessions = parse_sessions("");
    assert!(sessions.is_empty());
}

#[test]
fn parse_sessions_handles_malformed_lines_with_defaults() {
    let output = "onlyname\n";
    let sessions = parse_sessions(output);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["name"], "onlyname");
    assert_eq!(sessions[0]["windows"], 0);
    assert_eq!(sessions[0]["created"], 0);
    assert_eq!(sessions[0]["attached"], 0);
}

#[test]
fn parse_windows_parses_pipe_delimited_lines() {
    let output = "0|bash|1|1\n1|editor|2|0\n";
    let windows = parse_windows(output);

    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0]["index"], 0);
    assert_eq!(windows[0]["name"], "bash");
    assert_eq!(windows[0]["panes"], 1);
    assert_eq!(windows[0]["active"], true);
    assert_eq!(windows[1]["index"], 1);
    assert_eq!(windows[1]["name"], "editor");
    assert_eq!(windows[1]["panes"], 2);
    assert_eq!(windows[1]["active"], false);
}

#[test]
fn parse_windows_handles_empty_output() {
    let windows = parse_windows("");
    assert!(windows.is_empty());
}

#[test]
fn parse_panes_parses_pipe_delimited_lines() {
    let output = "%0|0|1|bash|main|80x24\n%1|1|0|vim|editor|120x40\n";
    let panes = parse_panes(output);

    assert_eq!(panes.len(), 2);
    assert_eq!(panes[0]["id"], "%0");
    assert_eq!(panes[0]["index"], 0);
    assert_eq!(panes[0]["active"], true);
    assert_eq!(panes[0]["command"], "bash");
    assert_eq!(panes[0]["title"], "main");
    assert_eq!(panes[0]["width"], 80);
    assert_eq!(panes[0]["height"], 24);
    assert_eq!(panes[1]["id"], "%1");
    assert_eq!(panes[1]["index"], 1);
    assert_eq!(panes[1]["active"], false);
    assert_eq!(panes[1]["command"], "vim");
    assert_eq!(panes[1]["width"], 120);
    assert_eq!(panes[1]["height"], 40);
}

#[test]
fn parse_panes_handles_empty_output() {
    let panes = parse_panes("");
    assert!(panes.is_empty());
}

#[test]
fn parse_panes_handles_malformed_size() {
    let output = "%0|0|1|bash|main|notasize\n";
    let panes = parse_panes(output);
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0]["width"], 0);
    assert_eq!(panes[0]["height"], 0);
}

#[test]
fn parse_size_parses_wxh() {
    assert_eq!(parse_size("80x24"), (80, 24));
    assert_eq!(parse_size("120x40"), (120, 40));
}

#[test]
fn parse_size_handles_malformed_input() {
    assert_eq!(parse_size(""), (0, 0));
    assert_eq!(parse_size("80"), (80, 0));
    assert_eq!(parse_size("abc"), (0, 0));
}

#[test]
fn parse_panes_find_filters_by_name() {
    let output = "%0|0|1|bash|main|80x24|/home/user\n%1|1|0|vim|editor|120x40|/home/user/proj\n";
    let panes = parse_panes_find(output, Some("main"), None);
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0]["id"], "%0");
}

#[test]
fn parse_panes_find_filters_by_context_path() {
    let output = "%0|0|1|bash|main|80x24|/home/user\n%1|1|0|vim|editor|120x40|/home/user/proj\n";
    let panes = parse_panes_find(output, None, Some("proj"));
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0]["id"], "%1");
}

#[test]
fn parse_panes_find_filters_by_context_command() {
    let output = "%0|0|1|bash|main|80x24|/home/user\n%1|1|0|vim|editor|120x40|/home/user\n";
    let panes = parse_panes_find(output, None, Some("vim"));
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0]["id"], "%1");
}

#[test]
fn parse_panes_find_returns_empty_when_no_match() {
    let output = "%0|0|1|bash|main|80x24|/home/user\n";
    let panes = parse_panes_find(output, Some("nonexistent"), None);
    assert!(panes.is_empty());
}

#[test]
fn parse_panes_find_with_no_filters_returns_all() {
    let output = "%0|0|1|bash|main|80x24|/home/user\n%1|1|0|vim|editor|120x40|/home/user\n";
    let panes = parse_panes_find(output, None, None);
    assert_eq!(panes.len(), 2);
}

#[test]
fn require_force_accepts_true() {
    assert!(require_force(Some(true)).is_ok());
}

#[test]
fn require_force_rejects_missing() {
    let err = require_force(None).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert_eq!(err.message, "kill requires force: true");
}

#[test]
fn require_force_rejects_false() {
    let err = require_force(Some(false)).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert_eq!(err.message, "kill requires force: true");
}

#[test]
fn resolve_dir_keeps_absolute_paths() {
    let cwd = Path::new("/work");
    let resolved = resolve_dir(Some("/abs/path"), cwd);
    assert_eq!(resolved.as_deref(), Some("/abs/path"));
}

#[test]
fn resolve_dir_joins_relative_paths() {
    let cwd = Path::new("/work");
    let resolved = resolve_dir(Some("sub/dir"), cwd);
    assert_eq!(resolved.as_deref(), Some("/work/sub/dir"));
}

#[test]
fn resolve_dir_returns_none_for_missing_directory() {
    let cwd = Path::new("/work");
    assert_eq!(resolve_dir(None, cwd), None);
}

#[test]
fn pane_target_with_percent_id_returns_id_directly() {
    assert_eq!(pane_target("nu-agent", Some("%5")), "%5");
}

#[test]
fn pane_target_with_window_name_returns_session_prefix() {
    assert_eq!(pane_target("nu-agent", Some("0")), "nu-agent:0");
}

#[test]
fn pane_target_without_pane_returns_session() {
    assert_eq!(pane_target("nu-agent", None), "nu-agent");
}
