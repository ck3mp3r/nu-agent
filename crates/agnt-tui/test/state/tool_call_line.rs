//! `ToolCallLine`: the identity the row adopts at construction.

use crate::state::{ToolCallLine, ToolCallStatus};
use agnt_runtime::protocol::events::ToolCallId;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_tool_call_line_new_keeps_the_fields_it_was_built_with() -> Result<()> {
    // -- Setup & Fixtures
    let key = ToolCallId::new("edit", "{}");

    // -- Exec
    let line = ToolCallLine::new(7, ToolCallStatus::InProgress, key.clone(), Some(3));

    // -- Check
    assert_eq!(line.id, 7);
    assert_eq!(line.status, ToolCallStatus::InProgress);
    assert_eq!(line.block_index, Some(3));
    assert_eq!(line.key(), &key);
    Ok(())
}

#[test]
fn test_tool_call_line_key_is_each_call_s_own_id() -> Result<()> {
    // -- Setup & Fixtures
    let first = ToolCallId::new("edit", "{\"path\":\"a\"}");
    let second = ToolCallId::new("nu", "{\"cmd\":\"ls\"}");

    // -- Exec
    let first_line = ToolCallLine::new(1, ToolCallStatus::InProgress, first.clone(), None);
    let second_line = ToolCallLine::new(2, ToolCallStatus::Done, second.clone(), Some(0));

    // -- Check
    assert_eq!(first_line.key().as_str(), first.as_str());
    assert_eq!(second_line.key().as_str(), second.as_str());
    assert_ne!(first_line.key(), second_line.key());
    Ok(())
}
