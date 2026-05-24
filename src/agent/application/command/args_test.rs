use crate::agent::application::command::args::extract_agent_flags;
use nu_plugin::EvaluatedCall;
use nu_protocol::{Span, Spanned, Value};

// Helper to create an EvaluatedCall with specific flags
fn mock_call_with_flags(agent: Option<&str>, name: Option<&str>) -> EvaluatedCall {
    let span = Span::test_data();
    
    let mut named: Vec<(Spanned<String>, Option<Value>)> = Vec::new();
    
    if let Some(agent_val) = agent {
        named.push((
            Spanned {
                item: "agent".to_string(),
                span,
            },
            Some(Value::string(agent_val, span)),
        ));
    }
    
    if let Some(name_val) = name {
        named.push((
            Spanned {
                item: "name".to_string(),
                span,
            },
            Some(Value::string(name_val, span)),
        ));
    }
    
    EvaluatedCall {
        head: span,
        positional: vec![],
        named,
    }
}

#[test]
fn extract_agent_flags_both_present() {
    let call = mock_call_with_flags(Some("developer"), Some("dev1"));
    let (agent, name) = extract_agent_flags(&call);
    
    assert_eq!(agent, Some("developer".to_string()));
    assert_eq!(name, Some("dev1".to_string()));
}

#[test]
fn extract_agent_flags_agent_only() {
    let call = mock_call_with_flags(Some("researcher"), None);
    let (agent, name) = extract_agent_flags(&call);
    
    assert_eq!(agent, Some("researcher".to_string()));
    // Name is None - no fallback, that's the caller's responsibility
    assert_eq!(name, None);
}

#[test]
fn extract_agent_flags_name_only() {
    let call = mock_call_with_flags(None, Some("custom-name"));
    let (agent, name) = extract_agent_flags(&call);
    
    assert_eq!(agent, None);
    assert_eq!(name, Some("custom-name".to_string()));
}

#[test]
fn extract_agent_flags_neither() {
    let call = mock_call_with_flags(None, None);
    let (agent, name) = extract_agent_flags(&call);
    
    assert_eq!(agent, None);
    assert_eq!(name, None);
}
