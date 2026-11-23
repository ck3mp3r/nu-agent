//! The manager builders, the path fixtures, and the store seeding.

use super::*;

/// The rig-memory store config the manager reads conversations from.
///
/// File-backed under `dir`, so every `open` reaches the same database.
pub(crate) fn rig_config(dir: &Path) -> RigStoreConfig {
    RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(dir)
}

/// The application paths of a session test rooted at `dir`.
///
/// The config file and the persona dir sit under `dir`, so the manager never
/// reads the user's real config or persona files.
pub(crate) fn test_paths(dir: &Path) -> agnt_runtime::config::Paths {
    agnt_runtime::config::Paths {
        config_path: dir.join("config.toml"),
        dotenv_path: dir.join(".env"),
        agents_dir: dir.join("agents"),
        copilot_token_dir: dir.join("github_copilot"),
        store_path: dir.join("sessions.db"),
        log_dir: dir.join("logs"),
        audit_log_path: dir.join("tool_audit.log"),
        secrets_path: dir.join("secrets.json"),
        models_cache_path: dir.join("models.json"),
        theme_path: dir.join("theme.json"),
        grammar_cache_dir: dir.join("tree-sitter"),
    }
}

pub(crate) async fn manager(dir: &Path) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    Ok(AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    ))
}

/// A manager whose runtime builder is `builder`, so a test can configure the
/// mock model's script and the chain's compaction threshold.
pub(crate) async fn manager_with_builder(
    dir: &Path,
    builder: MockRuntimeBuilder,
) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    Ok(AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        builder,
    ))
}

pub(crate) fn session_cwd(dir: &Path, name: &str) -> Result<PathBuf> {
    let cwd = dir.join(name);
    std::fs::create_dir_all(&cwd)?;
    Ok(cwd)
}

/// A manager whose runtime builder uses `bus` for every session it builds.
///
/// Lets a test subscribe to the session channel before a session is opened, so
/// the `LifecycleEvent` the manager publishes is not missed.
pub(crate) async fn manager_with_bus(
    dir: &Path,
    bus: Bus,
) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    Ok(AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            bus: Some(bus),
            ..MockRuntimeBuilder::default()
        },
    ))
}

/// A manager whose runtime builder uses `bus`, plus the rig-memory store config
/// it reads conversations from.
pub(crate) async fn manager_with_bus_and_rig_store(
    dir: &Path,
    bus: Bus,
) -> Result<(AcpSessionManager<MockRuntimeBuilder>, RigStoreConfig)> {
    let rig = rig_config(dir);
    let manager = AcpSessionManager::with_runtime_builder(
        rig.clone(),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            bus: Some(bus),
            ..MockRuntimeBuilder::default()
        },
    );
    Ok((manager, rig))
}

/// A manager whose builder seeds `input`/`output` tokens into the runtime of
/// `session_id`, so two sessions can carry distinct cumulative totals.
pub(crate) async fn manager_with_usage_for_session(
    dir: &Path,
    bus: Bus,
    session_id: &str,
    input: u64,
    output: u64,
) -> Result<(AcpSessionManager<MockRuntimeBuilder>, RigStoreConfig)> {
    let rig = rig_config(dir);
    let manager = AcpSessionManager::with_runtime_builder(
        rig.clone(),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            bus: Some(bus),
            ..MockRuntimeBuilder::default()
        }
        .with_usage_for_session(session_id, input, output),
    );
    Ok((manager, rig))
}

/// A manager plus the rig-memory store config it reads conversations from, so a
/// test can seed a persisted session.
pub(crate) async fn manager_with_rig_store(
    dir: &Path,
) -> Result<(AcpSessionManager<MockRuntimeBuilder>, RigStoreConfig)> {
    let rig = rig_config(dir);
    let manager = AcpSessionManager::with_runtime_builder(
        rig.clone(),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    );
    Ok((manager, rig))
}

/// Persist a two-message conversation under `session_id` in the rig store.
pub(crate) async fn seed_session(rig: &RigStoreConfig, session_id: &str) -> Result<()> {
    let store = rig
        .open(session_id)
        .await
        .map_err(|e| format!("should open rig store: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, session_id)
        .map_err(|e| format!("should load rig conversation: {e:?}"))?;
    conversation.extend([
        Message::user("hello".to_string()),
        Message::assistant("hi there".to_string()),
    ]);
    rig_memory::save(&*store, session_id, &mut conversation)
        .map_err(|e| format!("should seed rig conversation: {e:?}"))?;
    Ok(())
}

/// A live in-memory `ConnectionTo<Client>` for tests that need a client handle.
///
/// The connection runs over an in-memory channel: no transport, no network, no
/// subprocess. The returned peer keeps the channel open for the test's lifetime.
pub(crate) async fn client_connection() -> Result<(ConnectionTo<Client>, Channel)> {
    let (channel, peer) = Channel::duplex();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = Agent
            .builder()
            .connect_with(channel, async move |cx| {
                let _ = tx.send(cx);
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
    });
    let connection = rx.await?;
    Ok((connection, peer))
}

/// A manager whose runtime builder records the `client_fs` of every build.
pub(crate) async fn manager_capturing_client_fs(
    dir: &Path,
) -> Result<(
    AcpSessionManager<MockRuntimeBuilder>,
    Arc<Mutex<Vec<Option<ClientFsState>>>>,
)> {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let manager = AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            captured_client_fs: Some(Arc::clone(&captured)),
            ..MockRuntimeBuilder::default()
        },
    );
    Ok((manager, captured))
}

/// A manager whose runtime builder records the `agent_name` of every build.
pub(crate) async fn manager_capturing_agent_name(
    dir: &Path,
) -> Result<(
    AcpSessionManager<MockRuntimeBuilder>,
    Arc<Mutex<Vec<Option<String>>>>,
)> {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let manager = AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            captured_agent_name: Some(Arc::clone(&captured)),
            ..MockRuntimeBuilder::default()
        },
    );
    Ok((manager, captured))
}
