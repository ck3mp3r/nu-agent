use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
pub(crate) struct GitRepoContext {
    git_dir: PathBuf,
    common_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HeadState {
    SymbolicRef(String),
    Detached(String),
}

impl HeadState {
    pub(super) fn branch_label(&self) -> Option<String> {
        match self {
            HeadState::SymbolicRef(reference) => {
                if let Some(branch) = reference.strip_prefix("refs/heads/") {
                    Some(branch.to_string())
                } else if reference.is_empty() {
                    None
                } else {
                    Some(reference.clone())
                }
            }
            HeadState::Detached(short_sha) => {
                if short_sha.is_empty() {
                    None
                } else {
                    Some(short_sha.clone())
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RepoBranchTracker {
    caller_cwd: Option<PathBuf>,
    repo_context: Option<GitRepoContext>,
    branch: Option<String>,
    watch_targets: Vec<PathBuf>,
}

/// One refresh's result: owned, so it can travel off the loop task.
#[derive(Debug, Clone, Default)]
pub(crate) struct BranchState {
    repo_context: Option<GitRepoContext>,
    branch: Option<String>,
    watch_targets: Vec<PathBuf>,
}

impl BranchState {
    /// Read the branch state for `caller_cwd`, starting from `repo_context` when
    /// it is already known.
    ///
    /// Blocking: it may run `git rev-parse` and it reads git ref files. The
    /// startup path runs it inline; the loop hands it to the blocking pool.
    pub(crate) fn compute(
        caller_cwd: Option<&Path>,
        repo_context: Option<&GitRepoContext>,
    ) -> Self {
        let Some(caller_cwd) = caller_cwd else {
            return Self::default();
        };
        let repo_context = match repo_context {
            Some(context) => Some(context.clone()),
            None => discover_git_repo_context(caller_cwd),
        };
        let Some(context) = repo_context.as_ref() else {
            return Self::default();
        };
        let head_state = read_head_state(context);
        let branch = head_state.as_ref().and_then(HeadState::branch_label);
        let watch_targets = watch_targets_for_context(context, head_state.as_ref());
        Self {
            repo_context,
            branch,
            watch_targets,
        }
    }
}

impl RepoBranchTracker {
    pub(crate) fn from_caller_cwd(caller_cwd: Option<PathBuf>) -> Self {
        let mut tracker = Self {
            caller_cwd,
            repo_context: None,
            branch: None,
            watch_targets: Vec::new(),
        };
        tracker.refresh();
        tracker
    }

    pub(crate) fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub(crate) fn caller_cwd(&self) -> Option<&Path> {
        self.caller_cwd.as_deref()
    }

    /// Paths that change when the branch switches (git HEAD, packed-refs, and
    /// the symbolic branch ref). The event watcher subscribes to these.
    pub(crate) fn watch_targets(&self) -> &[PathBuf] {
        &self.watch_targets
    }

    /// Re-read the current branch, inline. The startup path uses it; the render
    /// loop uses [`Self::refresh_off_loop`] instead.
    pub(crate) fn refresh(&mut self) {
        let state = self.compute_state();
        self.adopt_state(state);
    }

    /// Re-read the current branch on the blocking pool, then install the result.
    ///
    /// The refresh reads git ref files — blocking file I/O the render loop must
    /// never run on its own task.
    pub(crate) async fn refresh_off_loop(&mut self) {
        let caller_cwd = self.caller_cwd.clone();
        let repo_context = self.repo_context.clone();
        let computed = tokio::task::spawn_blocking(move || {
            BranchState::compute(caller_cwd.as_deref(), repo_context.as_ref())
        })
        .await;
        match computed {
            Ok(state) => self.adopt_state(state),
            Err(join_error) => log::warn!("git branch refresh task failed: {join_error}"),
        }
    }

    /// This tracker's next state. Blocking: callers on the render loop hand it
    /// to the blocking pool instead of running it inline.
    fn compute_state(&self) -> BranchState {
        BranchState::compute(self.caller_cwd.as_deref(), self.repo_context.as_ref())
    }

    /// Install one computed state.
    fn adopt_state(&mut self, state: BranchState) {
        self.repo_context = state.repo_context;
        self.branch = state.branch;
        self.watch_targets = state.watch_targets;
    }
}

pub(crate) fn discover_git_repo_context(cwd: &Path) -> Option<GitRepoContext> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let git_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());
    if git_dir.as_os_str().is_empty() {
        return None;
    }

    let common_dir = resolve_common_dir(&git_dir);
    Some(GitRepoContext {
        git_dir,
        common_dir,
    })
}

fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let commondir_path = git_dir.join("commondir");
    let Ok(raw) = fs::read_to_string(commondir_path) else {
        return git_dir.to_path_buf();
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return git_dir.to_path_buf();
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else {
        git_dir.join(path)
    }
}

pub(crate) fn read_head_state(context: &GitRepoContext) -> Option<HeadState> {
    let head_raw = fs::read_to_string(context.git_dir.join("HEAD")).ok()?;
    let head = head_raw.trim();
    if head.is_empty() {
        return None;
    }

    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        if reference.is_empty() {
            return None;
        }
        return Some(HeadState::SymbolicRef(reference.to_string()));
    }

    let short = head
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .take(12)
        .collect::<String>();

    if short.len() >= 7 {
        Some(HeadState::Detached(short))
    } else {
        None
    }
}

fn watch_targets_for_context(
    context: &GitRepoContext,
    head_state: Option<&HeadState>,
) -> Vec<PathBuf> {
    let mut targets = BTreeSet::new();
    targets.insert(context.git_dir.join("HEAD"));
    targets.insert(context.common_dir.join("packed-refs"));

    if let Some(HeadState::SymbolicRef(reference)) = head_state {
        targets.insert(context.git_dir.join(reference));
        targets.insert(context.common_dir.join(reference));
    }

    targets.into_iter().collect()
}
