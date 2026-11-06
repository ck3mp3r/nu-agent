use super::{
    pane_target, parse_panes, parse_panes_find, parse_sessions, parse_size, parse_windows,
    require_force, resolve_dir,
};
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[test]
fn parse_sessions_table() {
    // -- Setup & Fixtures
    type Case = (
        &'static str,
        &'static str,
        Vec<(&'static str, u64, u64, u64)>,
    );

    let cases: Vec<Case> = vec![
        (
            "pipe delimited lines",
            "main|3|1700000000|1\nscratch|1|1700000100|0\n",
            vec![("main", 3, 1700000000, 1), ("scratch", 1, 1700000100, 0)],
        ),
        ("empty output", "", vec![]),
        (
            "malformed line uses defaults",
            "onlyname\n",
            vec![("onlyname", 0, 0, 0)],
        ),
    ];

    // -- Exec & Check
    for (name, output, expected) in cases {
        let sessions = parse_sessions(output);
        assert_eq!(
            sessions.len(),
            expected.len(),
            "case '{name}': session count"
        );
        for (session, (session_name, windows, created, attached)) in sessions.iter().zip(expected) {
            assert_eq!(session["name"], session_name, "case '{name}': name");
            assert_eq!(session["windows"], windows, "case '{name}': windows");
            assert_eq!(session["created"], created, "case '{name}': created");
            assert_eq!(session["attached"], attached, "case '{name}': attached");
        }
    }
}

#[test]
fn parse_windows_table() {
    // -- Setup & Fixtures
    type Case = (
        &'static str,
        &'static str,
        Vec<(u64, &'static str, u64, bool)>,
    );

    let cases: Vec<Case> = vec![
        (
            "pipe delimited lines",
            "0|bash|1|1\n1|editor|2|0\n",
            vec![(0, "bash", 1, true), (1, "editor", 2, false)],
        ),
        ("empty output", "", vec![]),
    ];

    // -- Exec & Check
    for (name, output, expected) in cases {
        let windows = parse_windows(output);
        assert_eq!(windows.len(), expected.len(), "case '{name}': window count");
        for (window, (index, window_name, panes, active)) in windows.iter().zip(expected) {
            assert_eq!(window["index"], index, "case '{name}': index");
            assert_eq!(window["name"], window_name, "case '{name}': name");
            assert_eq!(window["panes"], panes, "case '{name}': panes");
            assert_eq!(window["active"], active, "case '{name}': active");
        }
    }
}

#[test]
fn parse_panes_table() {
    // -- Setup & Fixtures
    type ExpectedPane = (
        &'static str,
        u64,
        u64,
        &'static str,
        bool,
        &'static str,
        &'static str,
        u64,
        u64,
    );
    let cases: Vec<(&str, &str, Vec<ExpectedPane>)> = vec![
        (
            "pipe delimited lines",
            "%0|0|1|bash|main|80x24\n%1|1|0|vim|editor|120x40\n",
            vec![
                ("%0", 0, 2, "nu-agent:2.0", true, "bash", "main", 80, 24),
                ("%1", 1, 2, "nu-agent:2.1", false, "vim", "editor", 120, 40),
            ],
        ),
        ("empty output", "", vec![]),
        (
            "malformed size uses zeros",
            "%0|0|1|bash|main|notasize\n",
            vec![("%0", 0, 2, "nu-agent:2.0", true, "bash", "main", 0, 0)],
        ),
    ];

    // -- Exec & Check
    for (name, output, expected) in cases {
        let panes = parse_panes(output, "nu-agent", "2");
        assert_eq!(panes.len(), expected.len(), "case '{name}': pane count");
        for (pane, (id, index, window, target, active, command, title, width, height)) in
            panes.iter().zip(expected)
        {
            assert_eq!(pane["id"], id, "case '{name}': id");
            assert_eq!(pane["index"], index, "case '{name}': index");
            assert_eq!(pane["window"], window, "case '{name}': window");
            assert_eq!(pane["target"], target, "case '{name}': target");
            assert_eq!(pane["active"], active, "case '{name}': active");
            assert_eq!(pane["command"], command, "case '{name}': command");
            assert_eq!(pane["title"], title, "case '{name}': title");
            assert_eq!(pane["width"], width, "case '{name}': width");
            assert_eq!(pane["height"], height, "case '{name}': height");
        }
    }
}

#[test]
fn parse_size_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, (u64, u64))> = vec![
        ("parses wxh", "80x24", (80, 24)),
        ("parses a second wxh", "120x40", (120, 40)),
        ("empty input", "", (0, 0)),
        ("missing height", "80", (80, 0)),
        ("non numeric", "abc", (0, 0)),
    ];

    // -- Exec & Check
    for (name, input, expected) in cases {
        assert_eq!(parse_size(input), expected, "case '{name}'");
    }
}

#[test]
fn parse_panes_find_table() {
    // -- Setup & Fixtures
    let output = "%0|0|1|bash|main|80x24|/home/user\n%1|1|0|vim|editor|120x40|/home/user/proj\n";
    type Case = (
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        Vec<(&'static str, &'static str)>,
    );

    let cases: Vec<Case> = vec![
        (
            "filters by name",
            Some("main"),
            None,
            vec![("%0", "nu-agent:2.0")],
        ),
        (
            "filters by context path",
            None,
            Some("proj"),
            vec![("%1", "nu-agent:2.1")],
        ),
        (
            "filters by context command",
            None,
            Some("vim"),
            vec![("%1", "nu-agent:2.1")],
        ),
        ("no match", Some("nonexistent"), None, vec![]),
        (
            "no filters returns all",
            None,
            None,
            vec![("%0", "nu-agent:2.0"), ("%1", "nu-agent:2.1")],
        ),
    ];

    // -- Exec & Check
    for (name, filter_name, context, expected) in cases {
        let panes = parse_panes_find(output, "nu-agent", "2", filter_name, context);
        assert_eq!(panes.len(), expected.len(), "case '{name}': pane count");
        for (pane, (id, target)) in panes.iter().zip(expected) {
            assert_eq!(pane["id"], id, "case '{name}': id");
            assert_eq!(pane["window"], 2, "case '{name}': window");
            assert_eq!(pane["target"], target, "case '{name}': target");
        }
    }
}

#[test]
fn require_force_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Option<bool>, bool)> = vec![
        ("accepts true", Some(true), true),
        ("rejects missing", None, false),
        ("rejects false", Some(false), false),
    ];

    // -- Exec & Check
    for (name, input, expected_ok) in cases {
        let result = require_force(input);
        assert_eq!(result.is_ok(), expected_ok, "case '{name}': ok");
        if let Err(err) = result {
            assert_eq!(err.kind, ToolErrorKind::Validation, "case '{name}': kind");
            assert_eq!(
                err.message, "kill requires force: true",
                "case '{name}': message"
            );
        }
    }
}

#[test]
fn resolve_dir_table() {
    // -- Setup & Fixtures
    let cwd = Path::new("/work");
    let cases: Vec<(&str, Option<&str>, Option<&str>)> = vec![
        ("keeps absolute paths", Some("/abs/path"), Some("/abs/path")),
        (
            "joins relative paths",
            Some("sub/dir"),
            Some("/work/sub/dir"),
        ),
        ("returns none for missing directory", None, None),
    ];

    // -- Exec & Check
    for (name, input, expected) in cases {
        assert_eq!(
            resolve_dir(input, cwd).as_deref(),
            expected,
            "case '{name}'"
        );
    }
}

#[test]
fn pane_target_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Option<&str>, Option<&str>, &str)> = vec![
        ("session only", None, None, "nu-agent"),
        ("session and window", Some("2"), None, "nu-agent:2"),
        (
            "session window and pane",
            Some("2"),
            Some("1"),
            "nu-agent:2.1",
        ),
        ("pane without window", None, Some("1"), "nu-agent:.1"),
        ("percent id ignores window", Some("2"), Some("%5"), "%5"),
    ];

    // -- Exec & Check
    for (name, window, pane, expected) in cases {
        assert_eq!(
            pane_target("nu-agent", window, pane),
            expected,
            "case '{name}'"
        );
    }
}
