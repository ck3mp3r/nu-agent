//! Client filesystem capability tests for `build_acp_runtime()`.

use agent_client_protocol::schema::v1::FileSystemCapabilities;
use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// client filesystem capabilities
// ---------------------------------------------------------------------------

/// A live in-memory `ConnectionTo<Client>` for a `ClientFsState`.
///
/// The connection runs over a duplex channel: no transport, no network, no
/// subprocess. The client task owns the peer endpoint, so the channel stays
/// open for the test's lifetime.
async fn client_connection() -> Result<ConnectionTo<Client>> {
    let (agent_channel, client_channel) = Channel::duplex();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = Agent
            .builder()
            .connect_with(agent_channel, async move |cx| {
                let _ = tx.send(cx);
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
    });
    let connection = rx.await?;
    tokio::spawn(async move {
        let _ = Client
            .builder()
            .name("agnt-acp-runtime-test-client")
            .connect_to(client_channel)
            .await;
    });
    Ok(connection)
}

fn client_fs_state(
    connection: ConnectionTo<Client>,
    capabilities: FileSystemCapabilities,
) -> ClientFsState {
    ClientFsState {
        connection,
        capabilities,
    }
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_registers_client_fs_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // Enable the info level so the capability-summary log arguments are
    // evaluated; without a logger the `log::info!` arguments are skipped.
    log::set_max_level(log::LevelFilter::Info);
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store_type();
    let connection = client_connection().await?;
    let capabilities = FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true);
    let mut params = make_params(temp.path().to_path_buf(), config_path, Vec::new(), store);
    params.client_fs = Some(client_fs_state(connection, capabilities));

    // -- Exec
    let runtime = build_acp_runtime(params)
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = visible_tool_names(&runtime);

    // -- Check
    for expected in ["read", "edit"] {
        assert!(
            names.iter().any(|name| name == expected),
            "client-fs '{expected}' tool must be registered; got: {names:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_client_fs_read_only_keeps_local_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store_type();
    let connection = client_connection().await?;
    // Only the read capability: the client cannot write, so the local
    // filesystem tools must stay in place.
    let capabilities = FileSystemCapabilities::new().read_text_file(true);
    let mut params = make_params(temp.path().to_path_buf(), config_path, Vec::new(), store);
    params.client_fs = Some(client_fs_state(connection, capabilities));

    // -- Exec
    let runtime = build_acp_runtime(params)
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = visible_tool_names(&runtime);

    // -- Check
    for expected in ["read", "edit"] {
        assert!(
            names.iter().any(|name| name == expected),
            "local '{expected}' tool must stay registered; got: {names:?}"
        );
    }
    Ok(())
}
