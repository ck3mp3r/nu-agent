use agnt_runtime::logging::{LogIdentity, LogRole, LogStore, resolved_level};
use nu_protocol::{LabeledError, Span};

// region:    --- Functions

/// Install this process's one instance log.
///
/// The first call in a process fixes the file name, the header lines, and the
/// level; a later call in the same process reuses the running sink and repeats
/// nothing. The sink is always a file: the state ladder resolves it at process
/// start, and a command never aborts because the log file cannot open.
pub(crate) fn install_logging(
    name: Option<String>,
    level: log::LevelFilter,
    _head: Span,
) -> Result<(), LabeledError> {
    // `off` is a sink mode: the logger stays at warn so the ring still gathers
    // warn and above, and the file target is silenced instead.
    let (level, silenced) = resolved_level(level);

    // Resolve the instance's log directory from the state ladder at process
    // start; a `[paths].state_home` value retargets the sink after config load.
    let store = match agnt_runtime::config::Paths::resolve_for_process(None) {
        Ok(paths) => LogStore::open(paths.log_dir),
        Err(_) => LogStore::fallback(),
    };
    // The instance's own file: `agent-{name}-{pid}.log` when the invocation has
    // a name, and `agent-{pid}.log` otherwise. The invocation is a one-shot
    // `run`, and the role travels in the file's header lines, never in its name.
    let identity = LogIdentity::new(LogRole::Run).with_name(name);
    let writer = store.start(&identity);
    LogStore::set_process_silenced(silenced);

    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .filter_level(level)
        .target(env_logger::Target::Pipe(Box::new(writer)))
        .format_timestamp_millis()
        .try_init();
    // Always force the global max level — try_init() is a no-op when a logger is
    // already registered (e.g. by nu-plugin internals), silently discarding our
    // filter_level. set_max_level() always wins.
    log::set_max_level(level);
    Ok(())
}

/// The global level `RUST_LOG` selects, defaulting to `info`.
pub(crate) fn environment_level() -> log::LevelFilter {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .build()
        .filter()
}

// endregion: --- Functions
