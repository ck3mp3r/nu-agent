use super::*;
use serde_json::json;

// ---------------------------------------------------------------------------
// A2aMethod
// ---------------------------------------------------------------------------

#[test]
fn a2a_method_all_variants_serialize() -> core::result::Result<(), Box<dyn std::error::Error>> {
    use serde_test::{Token, assert_tokens};

    let cases: &[(A2aMethod, &str)] = &[
        (A2aMethod::TasksSend, "tasks.send"),
        (A2aMethod::TasksGet, "tasks.get"),
        (A2aMethod::TasksCancel, "tasks.cancel"),
        (A2aMethod::TasksSendStream, "tasks.sendStream"),
        (A2aMethod::AgentGetCard, "agent.getCard"),
    ];

    for (method, expected) in cases {
        assert_tokens(method, &[Token::Str(expected)]);
    }
    Ok(())
}

#[test]
fn a2a_method_display() -> core::result::Result<(), Box<dyn std::error::Error>> {
    assert_eq!(A2aMethod::TasksSend.to_string(), "tasks.send");
    assert_eq!(A2aMethod::TasksGet.to_string(), "tasks.get");
    assert_eq!(A2aMethod::TasksCancel.to_string(), "tasks.cancel");
    assert_eq!(A2aMethod::TasksSendStream.to_string(), "tasks.sendStream");
    assert_eq!(A2aMethod::AgentGetCard.to_string(), "agent.getCard");
    Ok(())
}

#[test]
fn a2a_method_try_from_valid() -> core::result::Result<(), Box<dyn std::error::Error>> {
    assert_eq!(A2aMethod::try_from("tasks.send")?, A2aMethod::TasksSend);
    assert_eq!(A2aMethod::try_from("tasks.get")?, A2aMethod::TasksGet);
    assert_eq!(A2aMethod::try_from("tasks.cancel")?, A2aMethod::TasksCancel);
    assert_eq!(
        A2aMethod::try_from("tasks.sendStream")?,
        A2aMethod::TasksSendStream
    );
    assert_eq!(
        A2aMethod::try_from("agent.getCard")?,
        A2aMethod::AgentGetCard
    );
    Ok(())
}

#[test]
fn a2a_method_try_from_invalid() -> core::result::Result<(), Box<dyn std::error::Error>> {
    assert!(A2aMethod::try_from("unknown.method").is_err());
    Ok(())
}

// ---------------------------------------------------------------------------
// JsonRpcRequest
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_request_roundtrip_with_params() -> core::result::Result<(), Box<dyn std::error::Error>>
{
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: "req-1".to_string(),
        a2a_method: A2aMethod::TasksSend,
        params: Some(json!({
            "id": "task-1",
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            }
        })),
    };

    let json = serde_json::to_value(&req)?;
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], "req-1");
    assert_eq!(json["method"], "tasks.send");
    assert!(json.get("params").is_some());

    let back: JsonRpcRequest = serde_json::from_value(json)?;
    assert_eq!(back, req);
    Ok(())
}

#[test]
fn json_rpc_request_roundtrip_without_params()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: "req-2".to_string(),
        a2a_method: A2aMethod::AgentGetCard,
        params: None,
    };

    let json = serde_json::to_value(&req)?;
    assert_eq!(json["method"], "agent.getCard");
    assert!(json.get("params").is_none());

    let back: JsonRpcRequest = serde_json::from_value(json)?;
    assert_eq!(back, req);
    Ok(())
}

#[test]
fn json_rpc_request_every_method() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let methods = [
        A2aMethod::TasksSend,
        A2aMethod::TasksGet,
        A2aMethod::TasksCancel,
        A2aMethod::TasksSendStream,
        A2aMethod::AgentGetCard,
    ];

    for method in &methods {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "req".to_string(),
            a2a_method: method.clone(),
            params: None,
        };
        let json = serde_json::to_value(&req)?;
        let back: JsonRpcRequest = serde_json::from_value(json)?;
        assert_eq!(back.a2a_method, *method);
    }
    Ok(())
}

#[test]
fn json_rpc_request_default_jsonrpc() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let req: JsonRpcRequest = serde_json::from_str(r#"{"id":"r1","method":"tasks.get"}"#)?;
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.id, "r1");
    assert_eq!(req.a2a_method, A2aMethod::TasksGet);
    Ok(())
}

// ---------------------------------------------------------------------------
// JsonRpcResponse
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_response_with_result() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: "resp-1".to_string(),
        result: Some(json!({"status": "ok"})),
        error: None,
    };

    let json = serde_json::to_value(&resp)?;
    assert_eq!(json["result"]["status"], "ok");
    assert!(json.get("error").is_none());

    let back: JsonRpcResponse = serde_json::from_value(json)?;
    assert_eq!(back, resp);
    Ok(())
}

#[test]
fn json_rpc_response_with_error() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: "resp-2".to_string(),
        result: None,
        error: Some(JsonRpcError {
            code: -32_000,
            message: "task not found".to_string(),
            data: None,
        }),
    };

    let json = serde_json::to_value(&resp)?;
    assert!(json.get("result").is_none());
    assert_eq!(json["error"]["code"], -32_000);

    let back: JsonRpcResponse = serde_json::from_value(json)?;
    assert_eq!(back, resp);
    Ok(())
}

#[test]
fn json_rpc_response_with_result_and_error() -> core::result::Result<(), Box<dyn std::error::Error>>
{
    // Per JSON-RPC 2.0 spec, having both result and error is unusual but
    // the struct allows it. Test documents the behavior.
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: "resp-3".to_string(),
        result: Some(json!("partial")),
        error: Some(JsonRpcError {
            code: -32_602,
            message: "Invalid params".to_string(),
            data: None,
        }),
    };

    let json = serde_json::to_value(&resp)?;
    assert_eq!(json["result"], "partial");
    assert_eq!(json["error"]["code"], -32_602);

    let back: JsonRpcResponse = serde_json::from_value(json)?;
    assert_eq!(back, resp);
    Ok(())
}

// ---------------------------------------------------------------------------
// JsonRpcNotification
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_notification_roundtrip() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let notif = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "tasks.status".to_string(),
        params: json!({"taskId": "t-1", "state": "working"}),
    };

    let json = serde_json::to_value(&notif)?;
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["method"], "tasks.status");
    assert_eq!(json["params"]["taskId"], "t-1");

    let back: JsonRpcNotification = serde_json::from_value(json)?;
    assert_eq!(back, notif);
    Ok(())
}
