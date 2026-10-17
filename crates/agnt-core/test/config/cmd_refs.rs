use std::sync::Mutex;

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A canned runner failure, rebuilt on every call.
enum Failure {
    Io(String),
    Exit { code: i32, stderr: String },
}

impl Failure {
    fn to_error(&self) -> CmdRunError {
        match self {
            Self::Io(message) => CmdRunError::Io(std::io::Error::other(message.clone())),
            Self::Exit { code, stderr } => CmdRunError::Exit {
                code: *code,
                stderr: StderrSuffix::new(stderr.clone()),
            },
        }
    }
}

/// What a [`MockRunner`] returns for every call.
enum Outcome {
    Stdout(String),
    Failure(Failure),
}

/// Records every call and returns a canned outcome. Spawns no process.
struct MockRunner {
    calls: Mutex<Vec<(String, Vec<String>)>>,
    outcome: Outcome,
}

impl MockRunner {
    fn returning(stdout: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcome: Outcome::Stdout(stdout.to_string()),
        }
    }

    fn failing(failure: Failure) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcome: Outcome::Failure(failure),
        }
    }

    fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl CmdRunner for MockRunner {
    fn run(&self, program: &str, args: &[&str]) -> core::result::Result<String, CmdRunError> {
        self.calls.lock().unwrap().push((
            program.to_string(),
            args.iter().map(|arg| arg.to_string()).collect(),
        ));
        match &self.outcome {
            Outcome::Stdout(text) => Ok(text.clone()),
            Outcome::Failure(failure) => Err(failure.to_error()),
        }
    }
}

fn parse(toml_src: &str) -> Result<toml::Value> {
    Ok(toml::from_str(toml_src)?)
}

fn string_at<'a>(value: &'a toml::Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}

fn expect_error(result: core::result::Result<(), CmdRefError>) -> Result<CmdRefError> {
    match result {
        Ok(()) => Err("a cmd: reference must not resolve".into()),
        Err(error) => Ok(error),
    }
}

// endregion: --- Test Support

// region:    --- Tests

#[test]
fn test_cmd_refs_resolve_table_value() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(
        r#"
[mcp.gh.env]
GITHUB_TOKEN = "cmd:gh auth token"
"#,
    )?;
    let runner = MockRunner::returning("ghp_secret\n");

    // -- Exec
    resolve_cmd_refs(&mut value, &runner)?;

    // -- Check
    assert_eq!(
        string_at(&value, &["mcp", "gh", "env", "GITHUB_TOKEN"]),
        Some("ghp_secret")
    );
    assert_eq!(
        runner.calls(),
        vec![(
            "gh".to_string(),
            vec!["auth".to_string(), "token".to_string()]
        )]
    );
    Ok(())
}

#[test]
fn test_cmd_refs_resolve_array_element() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(
        r#"
[mcp.foo]
args = ["--token", "cmd:op read 'OpenAI API Key'"]
"#,
    )?;
    let runner = MockRunner::returning("sk-op\n");

    // -- Exec
    resolve_cmd_refs(&mut value, &runner)?;

    // -- Check
    let args = value
        .get("mcp")
        .and_then(|v| v.get("foo"))
        .and_then(|v| v.get("args"))
        .and_then(toml::Value::as_array)
        .ok_or("args must be an array")?;
    assert_eq!(args[0].as_str(), Some("--token"));
    assert_eq!(args[1].as_str(), Some("sk-op"));
    assert_eq!(
        runner.calls(),
        vec![(
            "op".to_string(),
            vec!["read".to_string(), "OpenAI API Key".to_string()]
        )]
    );
    Ok(())
}

#[test]
fn test_cmd_refs_does_not_chain_resolved_output() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(r#"token = "cmd:echo cmd:echo hi""#)?;
    let runner = MockRunner::returning("cmd:echo hi\n");

    // -- Exec
    resolve_cmd_refs(&mut value, &runner)?;

    // -- Check
    assert_eq!(string_at(&value, &["token"]), Some("cmd:echo hi"));
    assert_eq!(
        runner.calls().len(),
        1,
        "a resolved value must not be resolved again"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_trims_trailing_newlines() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(r#"token = "cmd:gh auth token""#)?;
    let runner = MockRunner::returning("ghp_secret\n\n");

    // -- Exec
    resolve_cmd_refs(&mut value, &runner)?;

    // -- Check
    assert_eq!(string_at(&value, &["token"]), Some("ghp_secret"));
    Ok(())
}

#[test]
fn test_cmd_refs_leaves_plain_values_untouched() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(
        r#"
plain = "store:openai"
other = "env:MY_KEY"
number = 3
"#,
    )?;
    let runner = MockRunner::returning("unused");

    // -- Exec
    resolve_cmd_refs(&mut value, &runner)?;

    // -- Check
    assert_eq!(string_at(&value, &["plain"]), Some("store:openai"));
    assert_eq!(string_at(&value, &["other"]), Some("env:MY_KEY"));
    assert!(
        runner.calls().is_empty(),
        "a config without cmd: values must not run a command"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_error_names_key_path_command_and_exit_code() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(
        r#"
[mcp.gh.env]
GITHUB_TOKEN = "cmd:gh auth token"
"#,
    )?;
    let runner = MockRunner::failing(Failure::Exit {
        code: 3,
        stderr: "not logged in".to_string(),
    });

    // -- Exec
    let error = expect_error(resolve_cmd_refs(&mut value, &runner))?;

    // -- Check
    let message = error.to_string();
    assert!(
        message.contains("mcp.gh.env.GITHUB_TOKEN"),
        "must name the key path: {message}"
    );
    assert!(
        message.contains("gh auth token"),
        "must name the command: {message}"
    );
    assert!(message.contains('3'), "must name the exit code: {message}");
    assert!(
        message.contains("not logged in"),
        "must include stderr: {message}"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_error_omits_empty_stderr() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(r#"token = "cmd:false""#)?;
    let runner = MockRunner::failing(Failure::Exit {
        code: 1,
        stderr: String::new(),
    });

    // -- Exec
    let error = expect_error(resolve_cmd_refs(&mut value, &runner))?;

    // -- Check
    let message = error.to_string();
    assert!(
        !message.contains("stderr"),
        "empty stderr must not appear: {message}"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_error_names_array_index_key_path() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(
        r#"
[mcp.foo]
args = ["--token", "cmd:false"]
"#,
    )?;
    let runner = MockRunner::failing(Failure::Io("boom".to_string()));

    // -- Exec
    let error = expect_error(resolve_cmd_refs(&mut value, &runner))?;

    // -- Check
    let message = error.to_string();
    assert!(
        message.contains("mcp.foo.args[1]"),
        "must name the array element: {message}"
    );
    assert!(
        message.contains("boom"),
        "must include the I/O error: {message}"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_error_names_top_level_array_index_key_path() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(r#"items = ["cmd:false"]"#)?;
    let runner = MockRunner::failing(Failure::Io("boom".to_string()));

    // -- Exec
    let error = expect_error(resolve_cmd_refs(&mut value, &runner))?;

    // -- Check
    let message = error.to_string();
    assert!(
        message.contains("items[0]"),
        "must name the array element: {message}"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_empty_output_errors() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(r#"token = "cmd:gh auth token""#)?;
    let runner = MockRunner::returning("\n");

    // -- Exec
    let error = expect_error(resolve_cmd_refs(&mut value, &runner))?;

    // -- Check
    assert!(
        matches!(&error, CmdRefError::Empty { .. }),
        "expected an empty-output error, got: {error}"
    );
    assert!(error.to_string().contains("token"));
    Ok(())
}

#[test]
fn test_cmd_refs_split_failure_errors() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(r#"token = "cmd:echo 'unbalanced""#)?;
    let runner = MockRunner::returning("unused");

    // -- Exec
    let error = expect_error(resolve_cmd_refs(&mut value, &runner))?;

    // -- Check
    assert!(
        matches!(&error, CmdRefError::Split { .. }),
        "expected a split error, got: {error}"
    );
    assert!(error.to_string().contains("token"));
    assert!(
        runner.calls().is_empty(),
        "an unsplittable reference must not run a command"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_empty_command_errors() -> Result<()> {
    // -- Setup & Fixtures
    let mut value = parse(r#"token = "cmd:   ""#)?;
    let runner = MockRunner::returning("unused");

    // -- Exec
    let error = expect_error(resolve_cmd_refs(&mut value, &runner))?;

    // -- Check
    assert!(
        matches!(&error, CmdRefError::EmptyCommand { .. }),
        "expected an empty-command error, got: {error}"
    );
    assert!(error.to_string().contains("token"));
    assert!(
        runner.calls().is_empty(),
        "an empty reference must not run a command"
    );
    Ok(())
}

// endregion: --- Tests
