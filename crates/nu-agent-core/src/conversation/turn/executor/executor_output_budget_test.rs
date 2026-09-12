//! OutputBudget tests: error surfacing and auto-raise on retry.

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_config};
use super::*;
use crate::config::Config;

/// An OutputBudget error must surface a user message naming `max_output_tokens` and
/// `--max-output-tokens`.
#[tokio::test]
async fn output_budget_error_surfaces_user_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget";
    let mut memory_state = make_memory_state(&temp_dir);

    // Four failing turns: attempts 1-3 each get one feedback retry (cap 3);
    // attempt 4 exceeds the cap, so the final break carries the OutputBudget
    // kind and the hard-error path surfaces the max_output_tokens message.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let err = result.expect_err("OutputBudget error must fail the turn");
    assert!(
        err.msg.contains("max_output_tokens"),
        "user message must mention max_output_tokens; got: {}",
        err.msg
    );
    assert!(
        err.msg.contains("--max-output-tokens"),
        "user message must mention --max-output-tokens; got: {}",
        err.msg
    );
    Ok(())
}

/// When raise is enabled and the effective max_tokens is below the cap, the
/// OutputBudget feedback retry runs the next attempt with max_tokens = base * multiplier.
#[tokio::test]
async fn output_budget_raise_applies_multiplier_on_retry() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        output_budget_raise_enabled: Some(true),
        output_budget_raise_multiplier: Some(2.0),
        output_budget_raise_cap: Some(32768),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise";
    let mut memory_state = make_memory_state(&temp_dir);

    // Attempt 1 fails with OutputBudget; attempt 2 succeeds.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the raised retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(
        requests[0].max_tokens,
        Some(1000),
        "first attempt keeps base"
    );
    assert_eq!(
        requests[1].max_tokens,
        Some(2000),
        "second attempt must carry raised max_tokens = 1000 * 2.0"
    );
    Ok(())
}

/// Two consecutive OutputBudget failures must compound the raise: the second
/// retry raises from the first raised value, not from the base config.
#[tokio::test]
async fn output_budget_raise_compounds_on_consecutive_failures() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        output_budget_raise_enabled: Some(true),
        output_budget_raise_multiplier: Some(2.0),
        output_budget_raise_cap: Some(32768),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-compound";
    let mut memory_state = make_memory_state(&temp_dir);

    // Attempts 1 and 2 fail with OutputBudget; attempt 3 succeeds.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the raised retries");
    let requests = spy.requests();
    assert_eq!(requests.len(), 3, "expected 3 model calls");
    assert_eq!(
        requests[0].max_tokens,
        Some(1000),
        "first attempt keeps base"
    );
    assert_eq!(
        requests[1].max_tokens,
        Some(2000),
        "second attempt must carry raised max_tokens = 1000 * 2.0"
    );
    assert_eq!(
        requests[2].max_tokens,
        Some(4000),
        "third attempt must compound the raise = 2000 * 2.0"
    );
    Ok(())
}

/// When raise is disabled (default), the OutputBudget feedback retry keeps the
/// unchanged effective max_tokens.
#[tokio::test]
async fn output_budget_raise_disabled_keeps_max_tokens_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-disabled";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, Some(1000));
    assert_eq!(
        requests[1].max_tokens,
        Some(1000),
        "disabled raise must keep max_tokens unchanged"
    );
    Ok(())
}

/// When raise is enabled but the effective max_tokens is None, no raise applies.
#[tokio::test]
async fn output_budget_raise_no_base_max_tokens_no_raise() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        output_budget_raise_enabled: Some(true),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-no-base";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, None);
    assert_eq!(
        requests[1].max_tokens, None,
        "no base max_tokens means no raise"
    );
    Ok(())
}

/// When raise is enabled and the base max_tokens is at or above the cap, no
/// raise applies.
#[tokio::test]
async fn output_budget_raise_base_at_cap_no_raise() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(32768),
        output_budget_raise_enabled: Some(true),
        output_budget_raise_cap: Some(32768),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-at-cap";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, Some(32768));
    assert_eq!(
        requests[1].max_tokens,
        Some(32768),
        "base at cap means no raise"
    );
    Ok(())
}

/// When raise is enabled but the kind is not OutputBudget (e.g. ToolStructure),
/// the feedback retry keeps max_tokens unchanged.
#[tokio::test]
async fn output_budget_raise_non_output_budget_kind_no_raise() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        output_budget_raise_enabled: Some(true),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-non-ob";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error(
            "invalid_request_body: tool_use call_id missing",
        )],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, Some(1000));
    assert_eq!(
        requests[1].max_tokens,
        Some(1000),
        "non-OutputBudget kind must not raise"
    );
    Ok(())
}

/// A mock turn that streams reasoning-only content then a final response with
/// `FinishReason::Length` must reach rig's empty-truncation check and surface as
/// a Prompt-wrapped `CompletionError::ResponseError` (the defect shape). The
/// executor must classify it as OutputBudget and run the feedback-retry path.
#[tokio::test]
async fn prompt_wrapped_empty_output_length_reaches_feedback_retry() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-prompt-wrapped-length";
    let mut memory_state = make_memory_state(&temp_dir);

    // Reasoning-only + truncating Length final: rig's `turn_delivered_no_answer`
    // returns true (reasoning is not an answer) and `truncating_finish_reason`
    // returns Length, so the run errors with the ResponseError message.
    let length_final = rig::streaming::StreamFinal::new("mock", rig::completion::Usage::new())
        .with_finish_reason(rig::completion::FinishReason::Length);
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let err = result.expect_err("empty-output Length must fail the turn");
    assert!(
        err.msg.contains("max_output_tokens"),
        "user message must mention max_output_tokens; got: {}",
        err.msg
    );
    assert!(
        err.msg.contains("--max-output-tokens"),
        "user message must mention --max-output-tokens; got: {}",
        err.msg
    );
    Ok(())
}
