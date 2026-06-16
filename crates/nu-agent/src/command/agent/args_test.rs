use super::args::{extract_agent_flags, extract_broker_flags};
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
fn mock_call_with_broker_flags(socket: Option<&str>, token: Option<&str>) -> EvaluatedCall {
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

// --- parse_strategy_from_str ---

use super::args::{extract_compaction_flags, parse_strategy_from_str};
use nu_agent_core::compaction::CompactionStrategy;

#[test]
fn parse_strategy_sliding_summary() {
    let result = parse_strategy_from_str("sliding_summary").unwrap();
    assert_eq!(result, CompactionStrategy::SlidingSummary);
}

#[test]
fn parse_strategy_sliding_window() {
    let result = parse_strategy_from_str("sliding_window").unwrap();
    assert_eq!(result, CompactionStrategy::SlidingWindow);
}

#[test]
fn parse_strategy_token_truncate() {
    let result = parse_strategy_from_str("token_truncate").unwrap();
    assert_eq!(result, CompactionStrategy::TokenTruncate);
}

#[test]
fn parse_strategy_alias_truncate() {
    let result = parse_strategy_from_str("truncate").unwrap();
    assert_eq!(result, CompactionStrategy::SlidingSummary);
}

#[test]
fn parse_strategy_invalid_rejected() {
    let result = parse_strategy_from_str("bogus");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Unknown compaction strategy 'bogus'"));
}

// --- extract_compaction_flags ---

fn mock_call_with_compaction_flags(
    strategy: Option<&str>,
    keep_recent: Option<i64>,
    token_budget: Option<i64>,
    proactive_pct: Option<f64>,
) -> EvaluatedCall {
    let span = Span::test_data();
    let mut named: Vec<(Spanned<String>, Option<Value>)> = Vec::new();

    if let Some(s) = strategy {
        named.push((
            Spanned {
                item: "compaction-strategy".to_string(),
                span,
            },
            Some(Value::string(s, span)),
        ));
    }
    if let Some(k) = keep_recent {
        named.push((
            Spanned {
                item: "keep-recent".to_string(),
                span,
            },
            Some(Value::int(k, span)),
        ));
    }
    if let Some(b) = token_budget {
        named.push((
            Spanned {
                item: "token-budget".to_string(),
                span,
            },
            Some(Value::int(b, span)),
        ));
    }
    if let Some(p) = proactive_pct {
        named.push((
            Spanned {
                item: "proactive-threshold-pct".to_string(),
                span,
            },
            Some(Value::float(p, span)),
        ));
    }

    EvaluatedCall {
        head: span,
        positional: vec![],
        named,
    }
}

#[test]
fn extract_compaction_flags_all_provided() {
    let call =
        mock_call_with_compaction_flags(Some("sliding_window"), Some(5), Some(10000), Some(0.75));
    let result = extract_compaction_flags(&call).unwrap();
    assert_eq!(result.strategy, Some(CompactionStrategy::SlidingWindow));
    assert_eq!(result.keep_recent, Some(5));
    assert_eq!(result.token_budget, Some(10000));
    assert_eq!(result.proactive_threshold_pct, Some(0.75));
}

#[test]
fn extract_compaction_flags_none_provided() {
    let call = mock_call_with_compaction_flags(None, None, None, None);
    let result = extract_compaction_flags(&call).unwrap();
    assert!(result.strategy.is_none());
    assert!(result.keep_recent.is_none());
    assert!(result.token_budget.is_none());
    assert!(result.proactive_threshold_pct.is_none());
}

#[test]
fn extract_compaction_flags_invalid_strategy_error() {
    let call = mock_call_with_compaction_flags(Some("bogus"), None, None, None);
    let result = extract_compaction_flags(&call);
    assert!(result.is_err());
}
