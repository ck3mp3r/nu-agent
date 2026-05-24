use nu_plugin::EvaluatedCall;
use nu_protocol::{LabeledError, Value};

/// Extracts and validates session flags from the evaluated call.
///
/// Returns the session_id as Option<String>.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing session flags
///
/// # Returns
/// An `Option<String>` representing the session ID.
///
/// # Errors
/// Returns an error if flags are invalid.
pub(crate) fn extract_and_validate_session_flags(
    call: &EvaluatedCall,
) -> Result<Option<String>, LabeledError> {
    // Extract flags
    let session_id = call.get_flag::<String>("session").ok().flatten();

    Ok(session_id)
}

/// Extract and parse closures from --tools flag.
///
/// Returns a HashMap of tool name to `Spanned<Closure>`, filtering out any non-closure values.
/// If the flag is not provided or is not a record, returns an empty HashMap.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing the --tools flag
///
/// # Returns
/// HashMap of tool names to spanned closures
pub(crate) fn extract_tools_from_call(
    call: &EvaluatedCall,
) -> Result<
    std::collections::HashMap<String, nu_protocol::Spanned<nu_protocol::engine::Closure>>,
    LabeledError,
> {
    use std::collections::HashMap;

    // Try to get --tools flag
    let tools_value: Option<Value> = call.get_flag("tools").ok().flatten();

    match tools_value {
        Some(Value::Record { val, .. }) => {
            // Filter and extract closures from the record
            let closures = val
                .iter()
                .filter_map(|(name, value)| {
                    if let Value::Closure {
                        val, internal_span, ..
                    } = value
                    {
                        // val is a Box<Closure>, need to deref and clone
                        // Wrap with span to preserve source location
                        Some((
                            name.to_string(),
                            nu_protocol::Spanned {
                                item: (**val).clone(),
                                span: *internal_span,
                            },
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            Ok(closures)
        }
        Some(_) => {
            // Non-record value provided - return empty HashMap (graceful handling)
            Ok(HashMap::new())
        }
        None => {
            // Flag not provided - return empty HashMap
            Ok(HashMap::new())
        }
    }
}

/// Extract and parse --tool-timeout flag.
///
/// Returns a Duration parsed from Nushell duration value (i64 nanoseconds).
/// If the flag is not provided, returns default of 30 seconds.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing the --tool-timeout flag
///
/// # Returns
/// Duration for tool execution timeout
pub(crate) fn extract_tool_timeout(call: &EvaluatedCall) -> std::time::Duration {
    // Extract the flag value (i64 nanoseconds)
    let timeout_nanos: Option<i64> = call.get_flag("tool-timeout").ok().flatten();

    // Convert to Duration, defaulting to 30 seconds
    timeout_nanos
        .map(|nanos| std::time::Duration::from_nanos(nanos as u64))
        .unwrap_or(std::time::Duration::from_secs(30))
}

/// Extract tool name patterns from --tool-filter flag.
///
/// Expected input is a list of strings, e.g. ["k8s__*", "gh__list_*"]
///
/// Returns an empty vector when the flag is not provided.
/// Empty vector means "no filtering" (match all tools).
pub(crate) fn extract_tool_filter_from_call(
    call: &EvaluatedCall,
) -> Result<Vec<String>, LabeledError> {
    let patterns_value: Option<Value> = call.get_flag("tool-filter").ok().flatten();

    let Some(value) = patterns_value else {
        return Ok(Vec::new());
    };

    let list = value.as_list().map_err(|_| {
        LabeledError::new("Invalid --tool-filter value")
            .with_label("--tool-filter must be a list of strings", value.span())
    })?;

    let mut patterns = Vec::with_capacity(list.len());
    for item in list {
        let pattern = item.as_str().map_err(|_| {
            LabeledError::new("Invalid --tool-filter entry")
                .with_label("Each --tool-filter entry must be a string", item.span())
        })?;
        patterns.push(pattern.to_string());
    }

    log::trace!("extract_tool_filter_from_call: patterns={patterns:?}");
    Ok(patterns)
}

/// Extract --agent and --name flags.
///
/// Returns raw (agent, name) values without fallback logic.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing the --agent and --name flags
///
/// # Returns
/// Tuple of (agent: Option<String>, name: Option<String>) - raw values without fallback
pub(crate) fn extract_agent_flags(call: &EvaluatedCall) -> (Option<String>, Option<String>) {
    let agent: Option<String> = call.get_flag("agent").ok().flatten();
    let name: Option<String> = call.get_flag("name").ok().flatten();
    log::trace!("extract_agent_flags: agent={agent:?}, name={name:?}");
    (agent, name)
}
