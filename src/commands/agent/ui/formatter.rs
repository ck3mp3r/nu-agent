use super::policy::Verbosity;

const TRACE_HARD_LIMIT: usize = 8192;
const VERY_VERBOSE_LIMIT: usize = 2048;
const VERBOSE_LIMIT: usize = 240;
const NORMAL_RESULT_LIMIT: usize = 120;

fn truncate_with_ellipsis(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    let mut s = input.chars().take(max).collect::<String>();
    s.push('…');
    s
}

pub fn format_tool_start(
    verbosity: Verbosity,
    name: &str,
    source: &str,
    arguments: &str,
) -> String {
    match verbosity {
        Verbosity::Quiet | Verbosity::Normal => format!("tool {name}"),
        Verbosity::Verbose => format!(
            "→ tool {name} ({source}) args={}",
            truncate_with_ellipsis(arguments, VERBOSE_LIMIT)
        ),
        Verbosity::VeryVerbose => format!(
            "→ tool {name} ({source})\nargs:\n{}",
            truncate_with_ellipsis(arguments, VERY_VERBOSE_LIMIT)
        ),
        Verbosity::Trace => format!(
            "→ tool {name} ({source})\nargs:\n{}",
            truncate_with_ellipsis(arguments, TRACE_HARD_LIMIT)
        ),
    }
}

pub struct ToolEndView<'a> {
    pub verbosity: Verbosity,
    pub name: &'a str,
    pub source: &'a str,
    pub arguments: &'a str,
    pub success: bool,
    pub result: &'a str,
    pub error_kind: Option<&'a str>,
    pub message: Option<&'a str>,
}

pub fn format_tool_end(view: ToolEndView<'_>) -> String {
    let ToolEndView {
        verbosity,
        name,
        source,
        arguments,
        success,
        result,
        error_kind,
        message,
    } = view;

    let status = if success { "✓" } else { "✗" };
    match verbosity {
        Verbosity::Quiet | Verbosity::Normal => {
            let header = format!(
                "{status} tool {name} args={}",
                truncate_with_ellipsis(arguments, NORMAL_RESULT_LIMIT)
            );
            if result.is_empty() {
                header
            } else {
                format!("{header}\n{}", truncate_with_ellipsis(result, NORMAL_RESULT_LIMIT))
            }
        }
        Verbosity::Verbose => {
            let header = format!(
                "{status} tool {name} ({source}) args={}",
                truncate_with_ellipsis(arguments, VERBOSE_LIMIT)
            );
            if result.is_empty() {
                header
            } else {
                format!("{header}\n{result}")
            }
        }
        Verbosity::VeryVerbose => {
            if success {
                let header = format!(
                    "{status} tool {name} ({source})\nargs:\n{}",
                    truncate_with_ellipsis(arguments, VERY_VERBOSE_LIMIT)
                );
                if result.is_empty() {
                    header
                } else {
                    format!(
                        "{header}\n{}",
                        truncate_with_ellipsis(result, VERY_VERBOSE_LIMIT)
                    )
                }
            } else {
                format!(
                    "{status} tool {name} ({source}) kind={} message={}\nargs:\n{}\n{}",
                    error_kind.unwrap_or("unknown"),
                    message.unwrap_or(""),
                    truncate_with_ellipsis(arguments, VERY_VERBOSE_LIMIT),
                    truncate_with_ellipsis(result, VERY_VERBOSE_LIMIT)
                )
            }
        }
        Verbosity::Trace => {
            if success {
                let header = format!(
                    "{status} tool {name} ({source})\nargs:\n{}",
                    truncate_with_ellipsis(arguments, TRACE_HARD_LIMIT)
                );
                if result.is_empty() {
                    header
                } else {
                    format!(
                        "{header}\n{}",
                        truncate_with_ellipsis(result, TRACE_HARD_LIMIT)
                    )
                }
            } else {
                format!(
                    "{status} tool {name} ({source}) kind={} message={}\nargs:\n{}\n{}",
                    error_kind.unwrap_or("unknown"),
                    message.unwrap_or(""),
                    truncate_with_ellipsis(arguments, TRACE_HARD_LIMIT),
                    truncate_with_ellipsis(result, TRACE_HARD_LIMIT)
                )
            }
        }
    }
}
