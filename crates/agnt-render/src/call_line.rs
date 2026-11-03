//! The call line: the one row a surface draws for a tool call, derived from the
//! call's name and raw arguments.
//!
//! Display, not data. A call's record carries the name and the raw arguments and
//! nothing else; every drawer derives the line from those, and no field, cache,
//! or record stores the result. Each tool's line lives with its view
//! ([`crate::tool_view`]), so a surface resolves the view once and calls
//! `call_line` on it — there is no second dispatch here.

use crate::tool_args::{
    call_line_from_json, nu_command_from_args, parse_json_string_field, parse_json_usize_field,
};
use agnt_kernel::builtin::NU_DEFAULT_TIMEOUT_SECONDS;

// region:    --- Support

/// `→ {path}`, or `→ {path}:{offset}-{end}` when the call names a window. The
/// end is the offset plus the limit, saturating like the read itself.
pub(crate) fn read(arguments: &str) -> String {
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
pub(crate) fn edit(arguments: &str) -> String {
    let Some(path) = parse_json_string_field(arguments, "path") else {
        return call_line_from_json(arguments);
    };
    format!("→ {path} (diff)")
}

/// `→ {pattern}`, joined with the directory the call searched in.
pub(crate) fn glob(arguments: &str) -> String {
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
pub(crate) fn grep(arguments: &str) -> String {
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
pub(crate) fn http(arguments: &str) -> String {
    let Some(url) = parse_json_string_field(arguments, "url") else {
        return call_line_from_json(arguments);
    };
    let method = parse_json_string_field(arguments, "method").unwrap_or_else(|| "GET".to_string());
    format!("→ {method} {url}")
}

/// `⏱ {timeout}s`: the command renders in the preview block, so the call line
/// carries only the time the command is allowed to run for.
pub(crate) fn nu(arguments: &str) -> String {
    if nu_command_from_args(arguments).is_none() {
        return call_line_from_json(arguments);
    }
    let timeout = parse_json_usize_field(arguments, "timeout_seconds")
        .unwrap_or(NU_DEFAULT_TIMEOUT_SECONDS as usize);
    format!("⏱ {timeout}s")
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/call_line.rs"]
mod call_line_test;

// endregion: --- Tests
