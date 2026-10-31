use super::*;

/// `PickerOption::from` forwards the model's configured flag, context
/// window, and cost, and the agent's description.
#[test]
fn picker_option_from_carries_payload_fields() -> Result<()> {
    // -- Setup & Fixtures
    let model =
        |configured: bool, context_window: Option<u32>, cost: Option<String>| ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
            context_window,
            max_output: None,
            configured,
            provider_display_name: String::new(),
            cost,
        };

    // -- Exec & Check: the configured flag is forwarded.
    for configured in [true, false] {
        let option = PickerOption::from(model(configured, None, None));
        match option.payload {
            PickerPayload::Model {
                configured: actual, ..
            } => assert_eq!(actual, configured, "configured flag must be forwarded"),
            _ => return Err("expected PickerPayload::Model".into()),
        }
    }

    // -- Exec & Check: the context window is forwarded.
    for context_window in [Some(200_000u32), None] {
        let option = PickerOption::from(model(true, context_window, None));
        match option.payload {
            PickerPayload::Model {
                context_window: actual,
                ..
            } => assert_eq!(actual, context_window),
            _ => return Err("expected PickerPayload::Model".into()),
        }
    }

    // -- Exec & Check: the cost is forwarded.
    for cost in [Some("$3.00/$15.00".to_string()), None] {
        let option = PickerOption::from(model(true, None, cost.clone()));
        match option.payload {
            PickerPayload::Model { cost: actual, .. } => assert_eq!(actual, cost),
            _ => return Err("expected PickerPayload::Model".into()),
        }
    }

    // -- Exec & Check: the agent description is forwarded.
    for description in [Some("Runs reviews".to_string()), None] {
        let opt = AgentPickerOption {
            name: "reviewer".into(),
            description: description.clone(),
            display: "reviewer".into(),
            builtin: false,
        };
        let option = PickerOption::from(opt);
        match option.payload {
            PickerPayload::Agent {
                description: actual,
                ..
            } => assert_eq!(actual, description),
            _ => return Err("expected PickerPayload::Agent".into()),
        }
    }
    Ok(())
}

/// The conversions declare the group each option belongs to: a model groups by
/// its provider, a session by its local day, an agent carries no group.
#[test]
fn picker_option_from_carries_the_section_key() -> Result<()> {
    // -- Setup & Fixtures
    let model = ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o-mini".to_string(),
        identity: "openai/gpt-4o-mini".to_string(),
        display: "openai / gpt-4o-mini".to_string(),
        active: false,
        context_window: None,
        max_output: None,
        configured: false,
        provider_display_name: "OpenAI".to_string(),
        cost: None,
    };
    let agent = AgentPickerOption {
        name: "reviewer".into(),
        description: None,
        display: "reviewer".into(),
        builtin: false,
    };
    let last_active = chrono::Utc::now();

    // -- Exec
    let model_section = PickerOption::from(model).section;
    let agent_section = PickerOption::from(agent).section;
    let session_section = PickerOption::from(agnt_core::session::SessionInfo {
        id: "abc123".to_string(),
        message_count: 3,
        last_active,
        title: Some("A session".to_string()),
    })
    .section;

    // -- Check
    assert_eq!(model_section.as_deref(), Some("openai"));
    assert_eq!(agent_section, None);
    assert_eq!(
        session_section,
        Some(crate::state::session_day(last_active).to_string())
    );
    Ok(())
}
