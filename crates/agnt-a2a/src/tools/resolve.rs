use crate::*;

/// Minimum accepted length of a task-ID prefix.
const MIN_PREFIX_LEN: usize = 8;

/// Resolve an exact task ID or a task-ID prefix to a full task ID.
///
/// The A2A contract does not constrain task-ID case, so a third-party server
/// may issue uppercase or mixed-case IDs. A full UUID is returned verbatim
/// with no HTTP request: the echoed string is the server's own ID and the
/// safest exact-lookup key for any server. Any other input must contain only
/// hex digits and dashes, must be at least [`MIN_PREFIX_LEN`] characters long,
/// and must match exactly one task ID of the target agent, comparing both
/// sides case-insensitively; otherwise an error string names the reason and,
/// for an ambiguous prefix, every candidate with its state.
pub(crate) async fn resolve_task_id<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
    input: &str,
) -> Result<String, String> {
    if uuid::Uuid::parse_str(input).is_ok() {
        return Ok(input.to_string());
    }

    if input.chars().any(|c| !(c.is_ascii_hexdigit() || c == '-')) {
        return Err(format!(
            "invalid task id '{input}': task ids contain hex digits and dashes only"
        ));
    }

    if input.chars().count() < MIN_PREFIX_LEN {
        return Err(format!(
            "task id prefix '{input}' is shorter than the minimum of {MIN_PREFIX_LEN} characters"
        ));
    }

    let tasks = list_all_tasks(client, target_url)
        .await
        .map_err(|e| format!("failed to list tasks: {e}"))?;

    let prefix = input.to_ascii_lowercase();
    let matches: Vec<Task> = tasks
        .into_iter()
        .filter(|task| task.id.to_ascii_lowercase().starts_with(&prefix))
        .collect();

    match matches.as_slice() {
        [task] => Ok(task.id.clone()),
        [] => Err(format!("no task found with id prefix '{input}'")),
        _ => {
            let candidates = matches
                .iter()
                .map(|task| format!("{} ({})", task.id, task.status.state.label()))
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "ambiguous task id prefix '{input}' matches {} tasks: {candidates}",
                matches.len()
            ))
        }
    }
}

// region:    --- Tests
#[cfg(test)]
#[path = "../../test/tools/resolve.rs"]
mod resolve_test;
// endregion: --- Tests
