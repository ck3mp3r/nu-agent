use super::*;

#[test]
fn turn_executor_new_constructs_without_panic() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let shared_model = super::test_utils::shared_mock_model();

    let _executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::Bus::default()),
    );
    // Construction succeeded — no panic.
    Ok(())
}

#[test]
fn turn_executor_exposes_memory_state() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let shared_model = super::test_utils::shared_mock_model();

    let executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::Bus::default()),
    );

    // Verify the executor carries the memory state it was built over.
    let _ = executor.memory_state.memory();
    Ok(())
}

#[test]
fn turn_executor_take_response_data_returns_none_before_execute() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let shared_model = super::test_utils::shared_mock_model();

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::Bus::default()),
    );

    assert!(executor.take_response_data().is_none());
    Ok(())
}
