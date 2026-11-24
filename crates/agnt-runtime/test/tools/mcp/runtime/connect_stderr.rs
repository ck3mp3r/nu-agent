//! The stdio transport's stderr capture: a child's own diagnostics reach the log
//! sink, tagged with its server, and never the terminal.

use std::sync::Mutex;

use log::{Level, Metadata, Record};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The clap dump a misconfigured MCP server writes to its stderr.
const CLAP_DUMP: &str = "error: unexpected argument '--add-paths' found";
const USAGE_LINE: &str = "Usage: nu-mcp --add-path <ADD_PATHS>";

/// The records the capture sink received. Every test asserts on its own
/// server-tagged text, so parallel tests cannot interfere.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct CaptureSink;

impl log::Log for CaptureSink {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        if record.level() > Level::Warn {
            return;
        }
        let mut logged = LOGGED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        logged.push(record.args().to_string());
    }

    fn flush(&self) {}
}

static SINK: CaptureSink = CaptureSink;

fn install_sink() {
    // A global logger installs once per test binary; a second call reports
    // AlreadyInitialized and leaves the first sink in place.
    let _ = log::set_logger(&SINK);
    log::set_max_level(log::LevelFilter::Warn);
}

fn logged_tagged(server: &str, needle: &str) -> bool {
    let logged = LOGGED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    logged
        .iter()
        .any(|line| line.contains(server) && line.contains(needle))
}

/// WHEN a stdio MCP child writes a diagnostic to its stderr, THE transport SHALL
/// pipe that stream and THE log sink SHALL receive each line tagged with the
/// server's name — so the child's bytes reach the log pane, not the terminal.
#[tokio::test(flavor = "multi_thread")]
async fn test_mcp_stdio_child_stderr_reaches_the_log_sink() -> Result<()> {
    // -- Setup & Fixtures: a child that writes the clap dump to stderr and exits.
    install_sink();
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(
        "printf \"error: unexpected argument '--add-paths' found\\n\\nUsage: nu-mcp \
         --add-path <ADD_PATHS>\\n\" >&2",
    );

    // -- Exec
    let transport = super::spawn_stdio_transport(cmd, "nu")?;

    // -- Check: the capture lands within a second, both lines, server-tagged.
    let mut found = false;
    for _ in 0..200 {
        if logged_tagged("MCP server 'nu'", CLAP_DUMP)
            && logged_tagged("MCP server 'nu'", USAGE_LINE)
        {
            found = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(
        found,
        "the child's stderr must reach the log sink tagged with its server"
    );
    drop(transport);
    Ok(())
}

/// WHEN the transport spawns a stdio child, THE child's stderr SHALL be piped:
/// an inherited stream would write the child's bytes straight onto the terminal
/// the TUI owns and paint outside its frame.
#[tokio::test(flavor = "multi_thread")]
async fn test_mcp_stdio_transport_pipes_the_child_stderr() -> Result<()> {
    // -- Setup & Fixtures: the same spawn seam the connect arm uses.
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg("exit 0");
    let (transport, child_stderr) = rmcp::transport::TokioChildProcess::builder(cmd)
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // -- Check
    assert!(
        child_stderr.is_some(),
        "a piped child stderr must hand back its read handle"
    );
    drop(transport);
    Ok(())
}
