use std::path::PathBuf;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

/// Spawns a filesystem watcher on git ref files and signals a channel whenever
/// a tracked file changes (e.g. an external `git checkout`).
///
/// This is event-driven: no polling. The render loop selects on the returned
/// receiver and refreshes the branch only when a change event actually fires.
pub(crate) fn spawn_branch_watcher(
    watch_targets: Vec<PathBuf>,
    signal_tx: mpsc::Sender<()>,
) -> Result<RecommendedWatcher, String> {
    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            if let Ok(_event) = result {
                let _ = signal_tx.try_send(());
            }
        },
        notify::Config::default(),
    )
    .map_err(|e| format!("failed to create git branch watcher: {e}"))?;

    for target in watch_targets {
        // Watch the parent dir so changes (create/delete/rename) to the ref
        // file are observed, not just in-place writes.
        let dir = target
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|e| format!("failed to watch {}: {e}", dir.display()))?;
    }

    Ok(watcher)
}
