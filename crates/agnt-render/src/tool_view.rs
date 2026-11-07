//! The tool view seam: one implementation per tool, resolved once.
//!
//! A tool is identity, input and output. What that looks like on screen is this
//! crate's business: each view builds its own call line and its own sections from
//! the call and the tool's payload. The dispatch happens once, in [`view_for`];
//! after it every call is a method on the view, so adding a tool never teaches a
//! downstream call site a new branch.

use agnt_kernel::{BuiltinKind, ToolCall, ToolKind};

use crate::call_line;
use crate::tool_args::{call_line_from_json, nu_command_from_args, parse_json_string_field};
use crate::transcript::ir::{ContentKind, Display, DisplaySection, DisplayStats};

// region:    --- Types

/// The rows a surface draws for a tool call.
///
/// A tool is identity, input and output. The drawer is the only layer that knows
/// how to turn those into rows, so the drawing lives here as an extension of the
/// call: every surface prints `call.call_line()`, and no surface matches a kind,
/// a name, or a string.
///
/// Dispatch is static: the concrete type is [`ToolCall`], known at every call
/// site, so nothing goes through a vtable. The kind itself is derived on use —
/// the call's name is the identity and a stored copy could drift from it.
pub trait ToolView {
    /// The single row a surface draws for the call, derived from the call alone.
    fn call_line(&self) -> String;

    /// The body a surface draws for the tool's result: `payload` is the data the
    /// handler itself returned. `None` when the tool draws no result body.
    fn result_body(&self, _payload: Option<&str>) -> Option<Display> {
        None
    }

    /// The card a surface draws before the call runs. `content` is the one datum
    /// only the producer can supply (an edit's proposed diff); everything else
    /// comes from the call's own arguments. `None` when the tool draws no card.
    fn preview_body(&self, _content: Option<&str>) -> Option<Display> {
        None
    }
}

// endregion: --- Types

// region:    --- Impls

impl ToolView for ToolCall {
    /// A tailored line for the builtins that have one; every other builtin and
    /// every tool from an open set — a closure or MCP tool — takes the generic
    /// arrow-prefixed JSON summary.
    fn call_line(&self) -> String {
        let arguments = self.arguments.as_str();
        match self.kind() {
            ToolKind::Builtin(BuiltinKind::Read) => call_line::read(arguments),
            ToolKind::Builtin(BuiltinKind::Edit) => call_line::edit(arguments),
            ToolKind::Builtin(BuiltinKind::Glob) => call_line::glob(arguments),
            ToolKind::Builtin(BuiltinKind::Grep) => call_line::grep(arguments),
            ToolKind::Builtin(BuiltinKind::Http) => call_line::http(arguments),
            ToolKind::Builtin(BuiltinKind::Nu) => call_line::nu(arguments),
            ToolKind::Builtin(_) | ToolKind::Other => call_line_from_json(arguments),
        }
    }

    fn result_body(&self, payload: Option<&str>) -> Option<Display> {
        match self.kind() {
            ToolKind::Builtin(BuiltinKind::Edit) => edit_body(self, payload),
            ToolKind::Builtin(_) | ToolKind::Other => None,
        }
    }

    fn preview_body(&self, content: Option<&str>) -> Option<Display> {
        match self.kind() {
            ToolKind::Builtin(BuiltinKind::Nu) => nu_card(self),
            ToolKind::Builtin(BuiltinKind::Edit) => edit_card(self, content),
            ToolKind::Builtin(_) | ToolKind::Other => None,
        }
    }
}

// endregion: --- Impls

// region:    --- Support

/// The completion body of an `edit`: the unified diff it made, with its stats
/// row. Every field comes out of the handler's own result payload.
fn edit_body(call: &ToolCall, payload: Option<&str>) -> Option<Display> {
    let payload: serde_json::Value = serde_json::from_str(payload?).ok()?;
    let path = payload
        .get("path")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| parse_json_string_field(&call.arguments, "path"))?;
    let diff = payload
        .get("diff")
        .and_then(|value| value.as_str())?
        .to_string();
    Some(diff_display(path, diff, payload.get("stats")))
}

/// The pre-run card of an `edit`: the producer's proposed diff, over the path
/// the call names.
fn edit_card(call: &ToolCall, content: Option<&str>) -> Option<Display> {
    let path = parse_json_string_field(&call.arguments, "path")?;
    Some(diff_display(path, content?.to_string(), None))
}

/// The pre-run card of a `nu` call: the command it is about to run. The command
/// is in the arguments, so no producer-side content is needed for it.
fn nu_card(call: &ToolCall) -> Option<Display> {
    let command = nu_command_from_args(&call.arguments)?;
    Some(Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: command,
            stats: None,
        }],
    })
}

/// One diff section over `path`, with the stats row the payload reported.
fn diff_display(path: String, diff: String, stats: Option<&serde_json::Value>) -> Display {
    Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path,
            },
            content: diff,
            stats: stats.and_then(parse_stats),
        }],
    }
}

/// The counters a result payload reports, when it reports any.
fn parse_stats(stats: &serde_json::Value) -> Option<DisplayStats> {
    let stats = stats.as_object()?;
    let read = |key: &str| {
        stats
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize)
    };
    Some(DisplayStats {
        files_changed: read("files_changed"),
        insertions: read("insertions"),
        deletions: read("deletions"),
        diff_truncated: stats
            .get("diff_truncated")
            .and_then(serde_json::Value::as_bool),
        omitted_files: read("omitted_files"),
        omitted_hunks: read("omitted_hunks"),
    })
}

// endregion: --- Support
