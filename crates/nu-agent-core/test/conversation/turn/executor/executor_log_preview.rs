//! Error-log preview byte-slice panic regression tests (multi-byte UTF-8).

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_config};
use super::*;
use crate::config::Config;

/// Hard-error path (executor.rs:560): a `CompletionFailed` with an
/// `Unknown` kind (non-retryable, not model-correctable) reaches
/// `log::error!("Turn failed with unrecoverable error: ...", &msg[..200])`.
/// With a multi-byte char straddling byte 200, the preview slice panicked
/// inside the error-handling path before the fix.
#[tokio::test]
async fn test_turn_executor_hard_error_log_preview_multibyte_utf8() -> Result<()> {
    // -- Setup & Fixtures
    install_error_logger();
    let config = Config {
        max_retries: Some(0),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().map_err(|e| format!("tempdir: {e:?}"))?;
    let session_id = "test-error-preview-multibyte";
    let mut memory_state = make_memory_state(&temp_dir);

    let error_text = multibyte_error_message();
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error(error_text.clone())]]);
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec & Check
    // Byte 200 of the error message lands inside the 2-byte 'é'; before the
    // fix the error-log preview sliced &msg[..msg.len().min(200)] and
    // panicked here — masking the original error.
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "run it".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;
    assert!(
        result.is_err(),
        "hard error must propagate as LabeledError to the caller"
    );
    let err = result.err().ok_or("should be an error")?;
    assert!(
        err.to_string().contains("rest of the provider error"),
        "error must carry the original provider text; got: {err}"
    );
    Ok(())
}

/// Path-A non-cancelled branch (executor.rs:404): a `MaxTurnsExceeded` where
/// the steering-retry cannot fire (no session id) falls through to
/// `let msg_preview = &msg[..msg.len().min(200)]` feeding
/// `log::error!("Turn error (path A non-cancelled): {msg_preview}")`.
/// With the error logger installed, this drives the same preview slice the
/// fix replaces — the turn must fail cleanly with the path-A error logged,
/// not panic. (The path-A message is rig's short max-turns text, so byte 200
/// is never reached here; the mid-char panic case is proven at the shared
/// hard-error site by `test_turn_executor_hard_error_log_preview_multibyte_utf8`.
/// Both sites get the identical `floor_char_boundary` replacement.)
#[tokio::test]
async fn test_turn_executor_path_a_error_log_preview_multibyte_utf8() -> Result<()> {
    // -- Setup & Fixtures
    install_error_logger();
    // max_tool_turns=0: rig raises MaxTurnsError as soon as a tool-call turn
    // would be scheduled. No session id → the steering-retry branch is
    // skipped (session-less turns cannot append feedback) and the path-A
    // preview + error log run immediately.
    let config = Config {
        max_tool_turns: Some(0),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().map_err(|e| format!("tempdir: {e:?}"))?;
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::tool_call(
        "tc1",
        "some_tool",
        serde_json::json!({"x": 1}),
    )]]);
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec & Check
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "run it".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "path-A hard error must propagate as LabeledError to the caller"
    );
    let err = result.err().ok_or("should be an error")?;
    assert!(
        err.to_string().contains("Max turns (0) exceeded"),
        "error must carry the max-turns text; got: {err}"
    );
    Ok(())
}
