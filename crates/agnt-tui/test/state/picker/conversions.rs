use super::*;

/// `PickerOption::from` forwards the model's configured flag and context
/// window, and the agent's description.
#[test]
fn picker_option_from_carries_payload_fields() -> Result<()> {
    // -- Setup & Fixtures
    let model = |configured: bool, context_window: Option<u32>| ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o-mini".to_string(),
        identity: "openai/gpt-4o-mini".to_string(),
        display: "openai / gpt-4o-mini".to_string(),
        active: true,
        context_window,
        max_output: None,
        configured,
        provider_display_name: String::new(),
    };

    // -- Exec & Check: the configured flag is forwarded.
    for configured in [true, false] {
        let option = PickerOption::from(model(configured, None));
        match option.payload {
            PickerPayload::Model {
                configured: actual, ..
            } => assert_eq!(actual, configured, "configured flag must be forwarded"),
            _ => return Err("expected PickerPayload::Model".into()),
        }
    }

    // -- Exec & Check: the context window is forwarded.
    for context_window in [Some(200_000u32), None] {
        let option = PickerOption::from(model(true, context_window));
        match option.payload {
            PickerPayload::Model {
                context_window: actual,
                ..
            } => assert_eq!(actual, context_window),
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
