use crate::agent::application::command::args::{extract_agent_flags, extract_broker_flags};
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

// Helper to create an EvaluatedCall with broker flags
fn mock_call_with_broker_flags(
    socket: Option<&str>,
    token: Option<&str>,
) -> EvaluatedCall {
    mock_call_with_broker_flags_and_parent(socket, token, None)
}

fn mock_call_with_broker_flags_and_parent(
    socket: Option<&str>,
    token: Option<&str>,
    parent_name: Option<&str>,
) -> EvaluatedCall {
    let span = Span::test_data();
    
    let mut named: Vec<(Spanned<String>, Option<Value>)> = Vec::new();
    
    if let Some(socket_val) = socket {
        named.push((
            Spanned {
                item: "broker-socket".to_string(),
                span,
            },
            Some(Value::string(socket_val, span)),
        ));
    }
    
    if let Some(token_val) = token {
        named.push((
            Spanned {
                item: "broker-token".to_string(),
                span,
            },
            Some(Value::string(token_val, span)),
        ));
    }

    if let Some(parent_val) = parent_name {
        named.push((
            Spanned {
                item: "parent-name".to_string(),
                span,
            },
            Some(Value::string(parent_val, span)),
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

#[test]
fn extract_broker_flags_both_present() {
    let call = mock_call_with_broker_flags(Some("/tmp/test.sock"), Some("secret-token"));
    let result = extract_broker_flags(&call).unwrap();
    
    assert!(result.is_some());
    let flags = result.unwrap();
    assert_eq!(flags.socket_path.to_str().unwrap(), "/tmp/test.sock");
    assert_eq!(flags.token, "secret-token");
}

#[test]
fn extract_broker_flags_neither() {
    let call = mock_call_with_broker_flags(None, None);
    let result = extract_broker_flags(&call).unwrap();
    
    assert!(result.is_none());
}

#[test]
fn extract_broker_flags_only_socket_errors() {
    let call = mock_call_with_broker_flags(Some("/tmp/test.sock"), None);
    let result = extract_broker_flags(&call);
    
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("must be used together"));
}

#[test]
fn extract_broker_flags_only_token_errors() {
    let call = mock_call_with_broker_flags(None, Some("secret-token"));
    let result = extract_broker_flags(&call);
    
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("must be used together"));
}

#[test]
fn extract_broker_flags_with_parent_name() {
    let call = mock_call_with_broker_flags_and_parent(
        Some("/tmp/test.sock"),
        Some("secret-token"),
        Some("my-parent"),
    );
    let result = extract_broker_flags(&call).unwrap();
    
    assert!(result.is_some());
    let flags = result.unwrap();
    assert_eq!(flags.socket_path.to_str().unwrap(), "/tmp/test.sock");
    assert_eq!(flags.token, "secret-token");
    assert_eq!(flags.parent_name, Some("my-parent".to_string()));
}

#[test]
fn extract_broker_flags_without_parent_name() {
    let call = mock_call_with_broker_flags(Some("/tmp/test.sock"), Some("secret-token"));
    let result = extract_broker_flags(&call).unwrap();
    
    let flags = result.unwrap();
    assert_eq!(flags.parent_name, None);
}
