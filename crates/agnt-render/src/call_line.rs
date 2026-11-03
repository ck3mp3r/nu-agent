//! The call line: the one row a surface draws for a tool call, derived from the
//! call's name and raw arguments.
//!
//! Display, not data. A call's record carries the name and the raw arguments and
//! nothing else; every drawer derives the line from those, and no field, cache,
//! or record stores the result. The mapping lives here because the layer that
//! paints the line owns it — the tool handlers keep execution.

use agnt_core::protocol::events::ToolCall;
use agnt_core::protocol::tool_args::{
    call_line_from_json, nu_command_from_args, parse_json_string_field, parse_json_usize_field,
};
use agnt_core::tools::handler::builtin_kinds::{BuiltinKind, ToolKind};
use agnt_core::tools::handler::nu::NuTool;

/// The line a surface draws for `call`.
///
/// A tailored line for the builtins that have one; every other builtin and every
/// tool from an open set — a closure or MCP tool — takes the generic
/// arrow-prefixed JSON summary. The match is total over [`BuiltinKind`], so a new
/// builtin does not compile until its line exists.
pub fn render_for(call: &ToolCall) -> String {
    let arguments = call.arguments.as_str();
    match call.kind() {
        ToolKind::Builtin(BuiltinKind::Read) => read(arguments),
        ToolKind::Builtin(BuiltinKind::Edit) => edit(arguments),
        ToolKind::Builtin(BuiltinKind::Glob) => glob(arguments),
        ToolKind::Builtin(BuiltinKind::Grep) => grep(arguments),
        ToolKind::Builtin(BuiltinKind::Http) => http(arguments),
        ToolKind::Builtin(BuiltinKind::Nu) => nu(arguments),
        ToolKind::Builtin(_) | ToolKind::Other => call_line_from_json(arguments),
    }
}

// region:    --- Support

/// `→ {path}`, or `→ {path}:{offset}-{end}` when the call names a window. The
/// end is the offset plus the limit, saturating like the read itself.
fn read(arguments: &str) -> String {
    let Some(path) = parse_json_string_field(arguments, "path") else {
        return call_line_from_json(arguments);
    };
    match (
        parse_json_usize_field(arguments, "offset"),
        parse_json_usize_field(arguments, "limit"),
    ) {
        (Some(offset), Some(limit)) => {
            format!("→ {path}:{offset}-{}", offset.saturating_add(limit))
        }
        _ => format!("→ {path}"),
    }
}

/// `→ {path} (diff)`: an edit always changes the file it names.
fn edit(arguments: &str) -> String {
    let Some(path) = parse_json_string_field(arguments, "path") else {
        return call_line_from_json(arguments);
    };
    format!("→ {path} (diff)")
}

/// `→ {pattern}`, joined with the directory the call searched in.
fn glob(arguments: &str) -> String {
    let Some(pattern) = parse_json_string_field(arguments, "pattern") else {
        return call_line_from_json(arguments);
    };
    let path = parse_json_string_field(arguments, "path").unwrap_or_else(|| ".".to_string());
    let combined = if path == "." {
        pattern
    } else {
        format!("{path}/{pattern}")
    };
    format!("→ {combined}")
}

/// `→ "{pattern}"` with the searched location appended when the call names one:
/// a bare directory, or a directory plus glob.
fn grep(arguments: &str) -> String {
    let Some(pattern) = parse_json_string_field(arguments, "pattern") else {
        return call_line_from_json(arguments);
    };
    let path = parse_json_string_field(arguments, "path").unwrap_or_else(|| ".".to_string());
    let glob = parse_json_string_field(arguments, "glob");

    let location = match (glob, path.as_str()) {
        (Some(glob), ".") => glob,
        (Some(glob), path) => format!("{path}/{glob}"),
        (None, ".") => String::new(),
        (None, path) => path.to_string(),
    };

    if location.is_empty() {
        format!("→ \"{pattern}\"")
    } else {
        format!("→ \"{pattern}\" {location}")
    }
}

/// `→ {method} {url}`, the method defaulting to GET.
fn http(arguments: &str) -> String {
    let Some(url) = parse_json_string_field(arguments, "url") else {
        return call_line_from_json(arguments);
    };
    let method = parse_json_string_field(arguments, "method").unwrap_or_else(|| "GET".to_string());
    format!("→ {method} {url}")
}

/// `⏱ {timeout}s`: the command renders in the preview block, so the call line
/// carries only the time the command is allowed to run for.
fn nu(arguments: &str) -> String {
    if nu_command_from_args(arguments).is_none() {
        return call_line_from_json(arguments);
    }
    let timeout = parse_json_usize_field(arguments, "timeout_seconds")
        .unwrap_or(NuTool::default_timeout_seconds() as usize);
    format!("⏱ {timeout}s")
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/call_line.rs"]
mod call_line_test;

// endregion: --- Tests
