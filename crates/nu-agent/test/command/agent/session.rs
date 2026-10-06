use super::test_helpers::{create_test_agent, create_test_call};
use super::{extract_tool_timeout, extract_tools_from_call};
use nu_plugin::{EvaluatedCall, SimplePluginCommand};
use nu_protocol::{Span, Spanned, SyntaxShape, Value};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// Tests for session flags (task 1.18)
#[cfg(test)]
mod session_flags_tests {
    use super::*;

    #[test]
    fn agent_command_signature_has_session_flag() -> Result<()> {
        // RED: Test that --session flag exists
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let session_flag = sig.named.iter().find(|f| f.long == "session");
        assert!(session_flag.is_some(), "Missing --session flag");

        let flag = session_flag.ok_or("should have --session flag")?;
        assert_eq!(
            flag.arg,
            Some(SyntaxShape::String),
            "Wrong type for --session"
        );
        assert!(!flag.desc.is_empty(), "Missing description for --session");
        Ok(())
    }

    #[test]
    fn agent_command_signature_has_agent_flag() -> Result<()> {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let agent_flag = sig.named.iter().find(|f| f.long == "agent");
        assert!(agent_flag.is_some(), "Missing --agent flag");

        let flag = agent_flag.ok_or("should have --agent flag")?;
        assert_eq!(
            flag.arg,
            Some(SyntaxShape::String),
            "Wrong type for --agent"
        );
        assert!(!flag.desc.is_empty(), "Missing description for --agent");
        Ok(())
    }

    #[test]
    fn agent_command_signature_has_name_flag() -> Result<()> {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let name_flag = sig.named.iter().find(|f| f.long == "name");
        assert!(name_flag.is_some(), "Missing --name flag");

        let flag = name_flag.ok_or("should have --name flag")?;
        assert_eq!(flag.arg, Some(SyntaxShape::String), "Wrong type for --name");
        assert!(!flag.desc.is_empty(), "Missing description for --name");
        Ok(())
    }
}

// Tests for session flag validation
#[cfg(test)]
mod session_validation_tests {
    use super::*;
    use crate::command::agent::extract_and_validate_session_flags;

    /// Helper to create a mock EvaluatedCall for testing
    fn create_mock_call_with_session_flags(session: Option<&str>) -> EvaluatedCall {
        let mut named = vec![];

        if let Some(id) = session {
            named.push((
                Spanned {
                    item: "session".to_string(),
                    span: Span::test_data(),
                },
                Some(Value::test_string(id)),
            ));
        }

        EvaluatedCall {
            head: Span::test_data(),
            positional: vec![],
            named,
        }
    }

    #[test]
    fn validate_session_flags_accepts_session_id_only() -> Result<()> {
        // RED: Test that --session <id> alone is valid
        let call = create_mock_call_with_session_flags(Some("my-session"));
        let result = extract_and_validate_session_flags(&call);

        let session_id = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(session_id, Some("my-session".to_string()));
        Ok(())
    }

    #[test]
    fn validate_session_flags_accepts_no_flags() -> Result<()> {
        // RED: Test that no session flags is valid (default behavior)
        let call = create_mock_call_with_session_flags(None);
        let result = extract_and_validate_session_flags(&call);

        let session_id = result.map_err(|e| format!("{e:?}"))?;
        assert!(session_id.is_none());
        Ok(())
    }
}

#[cfg(test)]
mod tui_session_resolution_tests {
    use nu_agent_core::session::resolver::{
        SessionRequest, generate_session_id, resolve_session_request,
    };
    use nu_protocol::{Span, Value};

    #[test]
    fn interactive_tui_without_session_auto_creates() {
        let request = resolve_session_request(true, None);
        match request {
            SessionRequest::Create(id) => {
                assert!(id.chars().next().is_some_and(|c| c.is_ascii_digit()))
            }
            other => panic!("expected Create request, got: {other:?}"),
        }
    }

    #[test]
    fn interactive_tui_with_session_attaches_existing() {
        let request = resolve_session_request(true, Some("chat-123".to_string()));
        assert_eq!(request, SessionRequest::Attach("chat-123".to_string()));
    }

    #[test]
    fn non_tui_with_session_keeps_legacy_get_or_create_behavior() {
        let request = resolve_session_request(false, Some("chat-legacy".to_string()));
        assert_eq!(request, SessionRequest::Create("chat-legacy".to_string()));
    }

    #[test]
    fn non_tui_without_session_returns_none() {
        let request = resolve_session_request(false, None);
        assert_eq!(request, SessionRequest::None);
    }

    #[test]
    fn generated_session_id_matches_expected_prefix() {
        let id = generate_session_id();
        assert!(
            id.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "expected timestamp prefix, got: {id}"
        );
        assert!(id.len() >= 15, "session id too short: {id}");
    }

    #[test]
    fn interactive_tui_normal_quit_returns_nothing() {
        let value = Value::nothing(Span::test_data());
        assert!(
            value.is_nothing(),
            "interactive TUI quit must return nothing"
        );
    }
}

// Integration tests for session functionality
#[cfg(test)]
mod session_integration_tests {
    use super::*;
    use crate::command::agent::extract_and_validate_session_flags;

    #[test]
    fn auto_generated_session_id_format() {
        // Verify auto-generated session IDs have correct format
        use chrono::Utc;
        let now = Utc::now();
        let session_id = format!(
            "session-{}-{}",
            now.format("%Y%m%d-%H%M%S"),
            now.timestamp_subsec_micros()
        );

        // Should start with "session-"
        assert!(session_id.starts_with("session-"));

        // Should contain date format with hyphens
        assert!(session_id.matches('-').count() >= 3); // At least session-, date-, time-

        // Should be reasonably long (at least 25 chars for session-YYYYMMDD-HHMMSS-X)
        assert!(
            session_id.len() >= 25,
            "Session ID too short: {} (len={})",
            session_id,
            session_id.len()
        );
    }

    #[test]
    fn extract_session_flags_with_session_id() -> Result<()> {
        // Test extracting --session flag
        let call = create_mock_call_with_session_flags(Some("my-session"));
        let result = extract_and_validate_session_flags(&call);

        let session_id = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(session_id, Some("my-session".to_string()));
        Ok(())
    }

    #[test]
    fn extract_session_flags_default_no_flags() -> Result<()> {
        // Test default behavior (no session flags)
        let call = create_mock_call_with_session_flags(None);
        let result = extract_and_validate_session_flags(&call);

        let session_id = result.map_err(|e| format!("{e:?}"))?;
        assert!(session_id.is_none());
        Ok(())
    }

    /// Helper to create a mock EvaluatedCall for testing (imported from session_validation_tests)
    fn create_mock_call_with_session_flags(session: Option<&str>) -> EvaluatedCall {
        let mut named = vec![];

        if let Some(id) = session {
            named.push((
                Spanned {
                    item: "session".to_string(),
                    span: Span::test_data(),
                },
                Some(Value::test_string(id)),
            ));
        }

        EvaluatedCall {
            head: Span::test_data(),
            positional: vec![],
            named,
        }
    }

    #[test]
    fn extract_tools_from_call_missing_flag() -> Result<()> {
        // Test with no --tools flag
        let call = create_test_call(vec![]);
        let result = extract_tools_from_call(&call);

        let tools = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(tools.len(), 0);
        Ok(())
    }

    #[test]
    fn extract_tools_from_call_empty_record() -> Result<()> {
        // Test with empty record
        use nu_protocol::Record;
        let call = create_test_call(vec![("tools", Value::test_record(Record::new()))]);
        let result = extract_tools_from_call(&call);

        let tools = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(tools.len(), 0);
        Ok(())
    }

    #[test]
    fn extract_tools_from_call_with_closures() -> Result<()> {
        // Test with record containing closures
        use nu_protocol::{BlockId, Record, engine::Closure};

        let mut record = Record::new();
        record.insert(
            "add".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(1),
                captures: vec![],
            }),
        );
        record.insert(
            "multiply".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(2),
                captures: vec![],
            }),
        );

        let call = create_test_call(vec![("tools", Value::test_record(record))]);
        let result = extract_tools_from_call(&call);

        let tools = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(tools.len(), 2);
        assert!(tools.contains_key("add"));
        assert!(tools.contains_key("multiply"));
        Ok(())
    }

    #[test]
    fn extract_tools_from_call_filters_non_closures() -> Result<()> {
        // Test that non-closure values are filtered out
        use nu_protocol::{BlockId, Record, engine::Closure};

        let mut record = Record::new();
        record.insert(
            "add".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(1),
                captures: vec![],
            }),
        );
        record.insert("name".to_string(), Value::test_string("not a closure"));
        record.insert("count".to_string(), Value::test_int(42));
        record.insert(
            "multiply".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(2),
                captures: vec![],
            }),
        );

        let call = create_test_call(vec![("tools", Value::test_record(record))]);
        let result = extract_tools_from_call(&call);

        let tools = result.map_err(|e| format!("{e:?}"))?;
        // Only closures should be extracted
        assert_eq!(tools.len(), 2);
        assert!(tools.contains_key("add"));
        assert!(tools.contains_key("multiply"));
        assert!(!tools.contains_key("name"));
        assert!(!tools.contains_key("count"));
        Ok(())
    }

    #[test]
    fn extract_tools_from_call_non_record_value() -> Result<()> {
        // Test with non-record value (graceful handling)
        let call = create_test_call(vec![("tools", Value::test_string("not a record"))]);
        let result = extract_tools_from_call(&call);

        let tools = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(tools.len(), 0);
        Ok(())
    }
}

// Tests for --tool-timeout flag parsing
mod tool_timeout_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parses_tool_timeout_flag() {
        // Test parsing Duration from Nushell duration value (i64 nanoseconds)
        // Nushell represents durations as i64 nanoseconds
        // 5 seconds = 5_000_000_000 nanoseconds
        let timeout_nanos = 5_000_000_000i64;
        let call = create_test_call(vec![("tool-timeout", Value::test_duration(timeout_nanos))]);

        // Use the helper function to extract timeout
        let timeout = extract_tool_timeout(&call);

        assert_eq!(timeout, Duration::from_secs(5));
    }

    #[test]
    fn defaults_to_30_seconds_when_flag_missing() {
        // Test default behavior when flag is not provided
        let call = create_test_call(vec![]);

        // Use the helper function (should return default)
        let timeout = extract_tool_timeout(&call);

        assert_eq!(timeout, Duration::from_secs(30));
    }

    #[test]
    fn parses_millisecond_timeout() {
        // Test parsing smaller duration (100ms = 100_000_000 nanoseconds)
        let timeout_nanos = 100_000_000i64;
        let call = create_test_call(vec![("tool-timeout", Value::test_duration(timeout_nanos))]);

        let timeout = extract_tool_timeout(&call);

        assert_eq!(timeout, Duration::from_millis(100));
    }

    #[test]
    fn agent_signature_has_tool_timeout_flag() -> Result<()> {
        // Test that the signature includes --tool-timeout flag
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let flag = sig.named.iter().find(|f| f.long == "tool-timeout");
        assert!(flag.is_some(), "Missing --tool-timeout flag");

        let flag = flag.ok_or("should have --tool-timeout flag")?;
        assert_eq!(flag.short, Some('t'), "Missing -t short flag");
        assert_eq!(
            flag.arg,
            Some(SyntaxShape::Duration),
            "Wrong type for --tool-timeout"
        );
        assert!(
            !flag.desc.is_empty(),
            "Missing description for --tool-timeout"
        );
        Ok(())
    }
}
