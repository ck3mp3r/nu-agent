use nu_plugin::EvaluatedCall;
use nu_protocol::{LabeledError, Value};
use std::path::PathBuf;

/// Broker connection flags
#[derive(Debug)]
pub(crate) struct BrokerFlags {
    pub socket_path: PathBuf,
    pub token: String,
    pub parent_name: Option<String>,
}

/// Extract --broker-socket and --broker-token flags.
///
/// Returns Some(BrokerFlags) if both flags are present, None if neither, Error if only one.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing the --broker-socket and --broker-token flags
///
/// # Returns
/// Option<BrokerFlags> - both present, None - neither present
///
/// # Errors
/// Returns LabeledError if only one flag is provided (both or neither required)
pub(crate) fn extract_broker_flags(
    call: &EvaluatedCall,
) -> Result<Option<BrokerFlags>, LabeledError> {
    let socket: Option<String> = call.get_flag("broker-socket").ok().flatten();
    let token: Option<String> = call.get_flag("broker-token").ok().flatten();
    let parent_name: Option<String> = call.get_flag("parent-name").ok().flatten();

    match (socket, token) {
        (Some(s), Some(t)) => Ok(Some(BrokerFlags {
            socket_path: PathBuf::from(s),
            token: t,
            parent_name,
        })),
        (None, None) => Ok(None),
        (Some(_), None) => Err(LabeledError::new(
            "--broker-socket and --broker-token must be used together",
        )),
        (None, Some(_)) => Err(LabeledError::new(
            "--broker-socket and --broker-token must be used together",
        )),
    }
}

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

/// Parse a compaction strategy string into a `CompactionStrategy` enum.
///
/// Uses serde deserialization which supports the canonical names and aliases:
/// - `"sliding_summary"` / `"truncate"` / `"sliding"` / `"summarize"` → `SlidingSummary`
/// - `"sliding_window"` → `SlidingWindow`
/// - `"token_truncate"` → `TokenTruncate`
///
/// # Arguments
/// * `s` - The strategy string to parse
///
/// # Returns
/// Ok(CompactionStrategy) if valid, Err(String) with user-facing message if invalid.
pub(crate) fn parse_strategy_from_str(
    s: &str,
) -> Result<crate::session::CompactionStrategy, String> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
        format!(
            "Unknown compaction strategy '{}'. Valid values: sliding_summary, sliding_window, token_truncate",
            s
        )
    })
}

/// Extract compaction-related CLI flags into a `CompactionConfig`.
///
/// All fields are `Option` — `None` means the flag was not provided.
/// The `--compaction-strategy` flag is parsed into a `CompactionStrategy` enum.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing the compaction flags
///
/// # Returns
/// Ok(CompactionConfig) with provided flag values, Err if strategy string is invalid.
///
/// # Errors
/// Returns LabeledError if `--compaction-strategy` contains an invalid strategy name.
pub(crate) fn extract_compaction_flags(
    call: &EvaluatedCall,
) -> Result<crate::config::CompactionConfig, LabeledError> {
    // Helper to safely extract usize flag (from i64, rejecting negatives)
    fn get_usize_flag(call: &EvaluatedCall, name: &str) -> Option<usize> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_int().ok())
            .and_then(|i| if i >= 0 { Some(i as usize) } else { None })
    }

    // Parse --compaction-strategy
    let strategy = if let Some(strategy_str) = call
        .get_flag::<Value>("compaction-strategy")
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(|s| s.to_string()).ok())
    {
        Some(parse_strategy_from_str(&strategy_str).map_err(|msg| {
            LabeledError::new("Invalid --compaction-strategy").with_label(msg, call.head)
        })?)
    } else {
        None
    };

    let threshold = get_usize_flag(call, "compaction-threshold");
    let keep_recent = get_usize_flag(call, "keep-recent");
    let token_budget = get_usize_flag(call, "token-budget");

    // Parse --proactive-threshold-pct
    let proactive_threshold_pct = call
        .get_flag::<Value>("proactive-threshold-pct")
        .ok()
        .flatten()
        .and_then(|v| v.as_float().ok());

    Ok(crate::config::CompactionConfig {
        strategy,
        threshold,
        keep_recent,
        token_budget,
        proactive_threshold_pct,
        fallback_strategies: None, // Not configurable via CLI
    })
}
