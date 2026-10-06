use crate::*;
use serde_json::Value;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn tool_def(name: &str) -> Result<A2aToolDef> {
    a2a_tool_defs()
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("should find tool {name}").into())
}

// ---------------------------------------------------------------------------
// Tool definition tests (sync)
// ---------------------------------------------------------------------------

#[test]
fn test_tool_defs_returns_six_tools() {
    let defs = a2a_tool_defs();
    assert_eq!(defs.len(), 6, "Should have 6 tool definitions");
}

#[test]
fn test_agent_list_has_no_parameters() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "agent_list")
        .ok_or("should find agent_list tool")?;
    let props = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        props.is_empty(),
        "agent_list should have no parameters (no filter) — LLM should get all agents"
    );
    Ok(())
}

#[test]
fn test_agent_get_card_requires_name() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "agent_getCard")
        .ok_or("should find agent_getCard tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("name".into())));
    Ok(())
}

#[test]
fn test_tasks_send_requires_target_and_text() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_send")
        .ok_or("should find tasks_send tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("target".into())));
    assert!(required.contains(&Value::String("text".into())));
    Ok(())
}

#[test]
fn test_tasks_send_schema_has_no_legacy_session_field() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_send")
        .ok_or("should find tasks_send tool")?;
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        !properties.contains_key("sessionId"),
        "tasks_send schema must not list sessionId"
    );
    Ok(())
}

#[test]
fn test_tasks_send_has_optional_context_id_param() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_send")
        .ok_or("should find tasks_send tool")?;
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        properties.contains_key("contextId"),
        "tasks_send should have optional contextId param"
    );
    assert_eq!(properties["contextId"]["type"], "string");

    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(
        !required.contains(&Value::String("contextId".into())),
        "contextId must be optional"
    );
    Ok(())
}

#[test]
fn test_tasks_get_requires_task_id_and_target() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_get")
        .ok_or("should find tasks_get tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("taskId".into())));
    assert!(required.contains(&Value::String("target".into())));
    Ok(())
}

#[test]
fn test_tasks_cancel_requires_task_id_and_target() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_cancel")
        .ok_or("should find tasks_cancel tool")?;
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(required.contains(&Value::String("taskId".into())));
    assert!(required.contains(&Value::String("target".into())));
    Ok(())
}

#[test]
fn test_tasks_list_has_optional_status_param() -> Result<()> {
    let defs = a2a_tool_defs();
    let tool = defs
        .iter()
        .find(|t| t.name == "tasks_list")
        .ok_or("should find tasks_list tool")?;
    assert!(
        tool.parameters.get("required").is_none(),
        "tasks.list should have no required params"
    );
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;
    assert!(
        properties.contains_key("status"),
        "tasks.list should have optional status param"
    );
    assert_eq!(properties["status"]["type"], "string");
    assert!(
        properties.contains_key("target"),
        "tasks.list should have optional target param"
    );

    // Every advertised status value must parse via TaskState::try_from, so the
    // schema and the strict parser cannot drift apart.
    let description = properties["status"]["description"]
        .as_str()
        .ok_or("status should have a description")?;
    let tokens: Vec<&str> = description
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .collect();
    let advertised: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| t.starts_with("TASK_STATE_"))
        .collect();
    assert!(
        !advertised.is_empty(),
        "status description should advertise TASK_STATE_* values, got: {description}"
    );
    for value in advertised {
        assert!(
            TaskState::try_from(value).is_ok(),
            "advertised status value {value} must parse via TaskState::try_from"
        );
    }
    Ok(())
}

#[test]
fn test_all_definitions_have_valid_json_schema() {
    for tool in a2a_tool_defs() {
        assert!(tool.parameters.get("type").is_some(), "missing type");
        assert!(
            tool.parameters.get("properties").is_some(),
            "missing properties"
        );
    }
}

#[test]
fn test_descriptions_avoid_banned_words() -> Result<()> {
    // -- Setup & Fixtures
    let banned = ["appropriate", "should", "etc.", "as needed"];

    // -- Exec & Check
    for tool in a2a_tool_defs() {
        let lower = tool.description.to_lowercase();
        for word in banned {
            assert!(
                !lower.contains(word),
                "{} description must not contain banned word {word:?}",
                tool.name
            );
        }
    }
    Ok(())
}

#[test]
fn test_descriptions_are_within_token_budget() -> Result<()> {
    // -- Setup & Fixtures
    const MAX_CHARS: usize = 500;

    // -- Exec & Check
    for tool in a2a_tool_defs() {
        assert!(
            tool.description.chars().count() <= MAX_CHARS,
            "{} description is {} chars, over the {MAX_CHARS} budget",
            tool.name,
            tool.description.chars().count()
        );
    }
    Ok(())
}

#[test]
fn test_tasks_send_description_documents_async_and_session() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_send")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(d.contains("asynchronously"), "must state async execution");
    assert!(d.contains("taskId"), "must name the taskId return");
    assert!(
        d.contains("conversation turn"),
        "must state completion arrives as a conversation turn"
    );
    assert!(
        d.contains("omit contextId") && d.contains("fresh session"),
        "must explain omitting contextId starts a fresh session"
    );
    assert!(
        d.contains("include contextId") && d.contains("resume"),
        "must explain including contextId resumes a session"
    );
    assert!(
        d.contains("lose all prior context"),
        "must state the consequence of omitting contextId"
    );
    assert!(
        d.contains("ALWAYS") && d.contains("contextId"),
        "must instruct the LLM to always reuse contextId on follow-up tasks"
    );
    assert!(
        d.contains("{taskId, contextId, status, message}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_tasks_get_description_documents_remote_and_shape() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_get")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("remote agent"),
        "must state the fetch is from a remote agent"
    );
    assert!(
        !d.contains("LOCAL"),
        "must not claim the task store is local"
    );
    assert!(
        d.contains("{taskId, state, artifacts}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_tasks_list_description_documents_both_modes() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_list")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("With target") && d.contains("remote agent"),
        "must document the remote mode"
    );
    assert!(
        d.contains("Without target") && d.contains("local store"),
        "must document the local mode"
    );
    for value in [
        "TASK_STATE_UNSPECIFIED",
        "TASK_STATE_SUBMITTED",
        "TASK_STATE_WORKING",
        "TASK_STATE_INPUT_REQUIRED",
        "TASK_STATE_COMPLETED",
        "TASK_STATE_FAILED",
        "TASK_STATE_CANCELED",
        "TASK_STATE_REJECTED",
        "TASK_STATE_AUTH_REQUIRED",
    ] {
        assert!(d.contains(value), "must list status value {value}");
    }
    assert!(
        d.contains("{tasks: [...]}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_tasks_cancel_description_documents_usage_and_shape() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_cancel")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("no longer needed") || d.contains("runs too long"),
        "must explain when to cancel"
    );
    assert!(
        d.contains("taskId") && d.contains("target") && d.contains("required"),
        "must state both taskId and target are required"
    );
    assert!(
        d.contains("{taskId, state}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_agent_list_description_documents_discovery_and_shape() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("agent_list")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("Call this first") && d.contains("discover"),
        "must state this is the discovery step"
    );
    assert!(
        d.contains("Excludes yourself"),
        "must state self is excluded"
    );
    assert!(
        d.contains("{agents: [{name, url, description, skills}]}"),
        "must document the response shape"
    );
    Ok(())
}

#[test]
fn test_agent_get_card_description_documents_contents_and_when() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("agent_getCard")?;

    // -- Exec & Check
    let d = &tool.description;
    assert!(
        d.contains("capabilities") && d.contains("skills") && d.contains("security"),
        "must describe what a card contains"
    );
    assert!(
        d.contains("agent_list already gives"),
        "must contrast with agent_list"
    );
    assert!(
        d.contains("{name, error}"),
        "must document the failure shape"
    );
    Ok(())
}

#[test]
fn test_tasks_list_status_schema_lists_all_values() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_list")?;
    let description = tool.parameters["properties"]["status"]["description"]
        .as_str()
        .ok_or("status should have a description")?;

    // -- Exec & Check
    for value in [
        "TASK_STATE_UNSPECIFIED",
        "TASK_STATE_SUBMITTED",
        "TASK_STATE_WORKING",
        "TASK_STATE_INPUT_REQUIRED",
        "TASK_STATE_COMPLETED",
        "TASK_STATE_FAILED",
        "TASK_STATE_CANCELED",
        "TASK_STATE_REJECTED",
        "TASK_STATE_AUTH_REQUIRED",
    ] {
        assert!(
            description.contains(value),
            "status schema must list {value}, got: {description}"
        );
    }
    Ok(())
}

#[test]
fn test_tasks_send_schema_context_id_is_optional_string() -> Result<()> {
    // -- Setup & Fixtures
    let tool = tool_def("tasks_send")?;
    let properties = tool.parameters["properties"]
        .as_object()
        .ok_or("should have properties object")?;

    // -- Exec & Check
    assert_eq!(properties["contextId"]["type"], "string");
    let required = tool.parameters["required"]
        .as_array()
        .ok_or("should have required array")?;
    assert!(
        !required.contains(&Value::String("contextId".into())),
        "contextId must be optional"
    );
    Ok(())
}
