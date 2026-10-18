//! ACP session/new config option test: the resolved model is advertised.

use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    InitializeRequest, SessionConfigKind, SessionConfigOptionCategory,
};

use super::*;

// ---------------------------------------------------------------------------
// session/new config options
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_new_session_advertises_model_config_option() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_text_response(&llm, "unused", None).await?;
    let env = TestEnv::new(&llm.uri(), "")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let options = build_client(recorder)
        .connect_with(env.agent(), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            session
                .config_options()
                .map(|options| options.to_vec())
                .ok_or_else(|| acp_err("session/new must advertise config options"))
        })
        .await?;

    // -- Check
    let model = options
        .iter()
        .find(|option| option.category == Some(SessionConfigOptionCategory::Model))
        .ok_or("a model config option must be advertised")?;
    let SessionConfigKind::Select(select) = &model.kind else {
        return Err("the model config option must be a select".into());
    };
    assert_eq!(
        select.current_value.to_string(),
        "ollama/llama3",
        "the advertised model must match the resolved config"
    );
    Ok(())
}
