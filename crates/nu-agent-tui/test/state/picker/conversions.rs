use super::*;

#[test]
fn from_model_picker_option_carries_configured_true() -> Result<()> {
    // -- Setup & Fixtures
    let opt = ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o-mini".to_string(),
        identity: "openai/gpt-4o-mini".to_string(),
        display: "openai / gpt-4o-mini".to_string(),
        active: true,
        context_window: None,
        max_output: None,
        configured: true,
        provider_display_name: String::new(),
    };

    // -- Exec
    let option = PickerOption::from(opt);

    // -- Check
    match option.payload {
        PickerPayload::Model { configured, .. } => {
            assert!(configured, "configured flag must be forwarded");
        }
        _ => return Err("expected PickerPayload::Model".into()),
    }
    Ok(())
}

#[test]
fn from_model_picker_option_carries_configured_false() -> Result<()> {
    // -- Setup & Fixtures
    let opt = ModelPickerOption {
        provider: "anthropic".to_string(),
        model: "claude-3-5-sonnet".to_string(),
        identity: "anthropic/claude-3-5-sonnet".to_string(),
        display: "anthropic / claude-3-5-sonnet".to_string(),
        active: false,
        context_window: None,
        max_output: None,
        configured: false,
        provider_display_name: String::new(),
    };

    // -- Exec
    let option = PickerOption::from(opt);

    // -- Check
    match option.payload {
        PickerPayload::Model { configured, .. } => {
            assert!(!configured, "configured flag must be forwarded");
        }
        _ => return Err("expected PickerPayload::Model".into()),
    }
    Ok(())
}

#[test]
fn from_agent_picker_option_carries_description_some() -> Result<()> {
    // -- Setup & Fixtures
    let opt = AgentPickerOption {
        name: "reviewer".into(),
        description: Some("Runs reviews".into()),
        display: "reviewer".into(),
        builtin: false,
    };

    // -- Exec
    let option = PickerOption::from(opt);

    // -- Check
    match option.payload {
        PickerPayload::Agent { description, .. } => {
            assert_eq!(description, Some("Runs reviews".to_string()));
        }
        _ => return Err("expected PickerPayload::Agent".into()),
    }
    Ok(())
}

#[test]
fn from_agent_picker_option_carries_description_none() -> Result<()> {
    // -- Setup & Fixtures
    let opt = AgentPickerOption {
        name: "reviewer".into(),
        description: None,
        display: "reviewer".into(),
        builtin: false,
    };

    // -- Exec
    let option = PickerOption::from(opt);

    // -- Check
    match option.payload {
        PickerPayload::Agent { description, .. } => {
            assert_eq!(description, None);
        }
        _ => return Err("expected PickerPayload::Agent".into()),
    }
    Ok(())
}

#[test]
fn from_model_picker_option_carries_context_window_some() -> Result<()> {
    // -- Setup & Fixtures
    let opt = ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        identity: "openai/gpt-4o".to_string(),
        display: "openai/gpt-4o".to_string(),
        active: true,
        context_window: Some(200000),
        max_output: None,
        configured: true,
        provider_display_name: String::new(),
    };

    // -- Exec
    let option = PickerOption::from(opt);

    // -- Check
    match option.payload {
        PickerPayload::Model { context_window, .. } => {
            assert_eq!(context_window, Some(200000));
        }
        _ => return Err("expected PickerPayload::Model".into()),
    }
    Ok(())
}

#[test]
fn from_model_picker_option_carries_context_window_none() -> Result<()> {
    // -- Setup & Fixtures
    let opt = ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        identity: "openai/gpt-4o".to_string(),
        display: "openai/gpt-4o".to_string(),
        active: true,
        context_window: None,
        max_output: None,
        configured: true,
        provider_display_name: String::new(),
    };

    // -- Exec
    let option = PickerOption::from(opt);

    // -- Check
    match option.payload {
        PickerPayload::Model { context_window, .. } => {
            assert_eq!(context_window, None);
        }
        _ => return Err("expected PickerPayload::Model".into()),
    }
    Ok(())
}
