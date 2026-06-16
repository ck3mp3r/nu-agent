use super::*;

#[test]
fn hook_event_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<HookEvent>();
}

#[test]
fn permission_decision_is_send_sync_clone() {
    fn assert_bounds<T: Send + Sync + Clone>() {}
    assert_bounds::<PermissionDecision>();
}

#[test]
fn can_construct_llm_start_event() {
    let _event = HookEvent::LlmStart;
}

#[test]
fn can_construct_ask_permission_event() {
    let (tx, _rx) = tokio::sync::oneshot::channel();
    let _event = HookEvent::AskPermission {
        tool_name: "read_file".to_string(),
        arguments: "{}".to_string(),
        tool_call_id: Some("call_123".to_string()),
        responder: tx,
    };
}
