use super::PendingOps;

#[test]
fn pending_ops_has_no_pending_when_new() {
    let ops = PendingOps::new();
    assert!(!ops.has_pending());
}

#[test]
fn queued_model_switch_makes_has_pending_true() {
    let mut ops = PendingOps::new();
    ops.queue_model_switch("gpt-4o".to_string());
    assert!(ops.has_pending());
}

#[test]
fn take_queued_model_switch_drains() {
    let mut ops = PendingOps::new();
    ops.queue_model_switch("gpt-4o".to_string());
    let spec = ops.take_queued_model_switch();
    assert_eq!(spec, Some("gpt-4o".to_string()));
    assert!(!ops.has_pending());
}

#[test]
fn take_queued_model_switch_twice_returns_none_second_time() {
    let mut ops = PendingOps::new();
    ops.queue_model_switch("gpt-4o".to_string());
    ops.take_queued_model_switch();
    assert_eq!(ops.take_queued_model_switch(), None);
}

#[test]
fn queued_agent_switch_makes_has_pending_true() {
    let mut ops = PendingOps::new();
    ops.queue_agent_switch("research".to_string());
    assert!(ops.has_pending());
}

#[test]
fn take_queued_agent_switch_drains() {
    let mut ops = PendingOps::new();
    ops.queue_agent_switch("research".to_string());
    let name = ops.take_queued_agent_switch();
    assert_eq!(name, Some("research".to_string()));
    assert!(!ops.has_pending());
}

#[test]
fn multiple_pending_ops_all_drained() {
    let mut ops = PendingOps::new();
    ops.queue_model_switch("gpt-4o".to_string());
    ops.queue_agent_switch("research".to_string());
    assert!(ops.has_pending());
    ops.take_queued_model_switch();
    ops.take_queued_agent_switch();
    assert!(!ops.has_pending());
}

#[test]
fn last_write_wins_for_model_switch() {
    let mut ops = PendingOps::new();
    ops.queue_model_switch("gpt-4o".to_string());
    ops.queue_model_switch("claude-4".to_string());
    assert_eq!(ops.take_queued_model_switch(), Some("claude-4".to_string()));
}
