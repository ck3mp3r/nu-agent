//! The coordinator's git branch surface: the tracker the status bar reads and the
//! loop refreshes off its own task.

use super::RuntimeCoordinator;
use crate::runtime::status::RepoBranchTracker;

impl RuntimeCoordinator {
    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<std::path::PathBuf>) {
        self.repo_branch_tracker = Some(RepoBranchTracker::from_caller_cwd(caller_cwd));
    }

    /// The git ref files that, when changed, indicate a branch switch. The
    /// render loop subscribes a filesystem watcher to these.
    pub(crate) fn repo_branch_watch_targets(&self) -> Vec<std::path::PathBuf> {
        self.repo_branch_tracker
            .as_ref()
            .map(|t| t.watch_targets().to_vec())
            .unwrap_or_default()
    }

    /// Re-read the current git branch after a filesystem change event.
    ///
    /// The read runs on the blocking pool: git ref reads are blocking file I/O
    /// and never run on the loop task.
    pub(crate) async fn refresh_repo_branch(&mut self) {
        if let Some(tracker) = self.repo_branch_tracker.as_mut() {
            tracker.refresh_off_loop().await;
        }
        self.mark_render_needed();
    }

    /// The current git branch shown in the status bar, if any.
    pub(crate) fn repo_branch(&self) -> Option<&str> {
        self.repo_branch_tracker.as_ref().and_then(|t| t.branch())
    }
}
