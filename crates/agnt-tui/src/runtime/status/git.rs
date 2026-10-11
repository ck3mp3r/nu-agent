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

impl RepoBranchTracker {
    pub(crate) fn from_caller_cwd(caller_cwd: Option<PathBuf>) -> Self {
        let mut tracker = Self {
            caller_cwd,
            repo_context: None,
            branch: None,
            watch_targets: Vec::new(),
        };
        tracker.refresh_branch_state();
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

    /// Re-read the current branch. Called when a filesystem event fires.
    pub(crate) fn refresh(&mut self) {
        self.refresh_branch_state();
    }

    fn refresh_branch_state(&mut self) {
        if self.caller_cwd.is_none() {
            self.repo_context = None;
            self.branch = None;
            self.watch_targets.clear();
            return;
        }

        if self.repo_context.is_none() {
            self.repo_context = self
                .caller_cwd
                .as_deref()
                .and_then(discover_git_repo_context);
        }

        let Some(context) = self.repo_context.as_ref() else {
            self.branch = None;
            self.watch_targets.clear();
            return;
        };

        let head_state = read_head_state(context);
        self.branch = head_state.as_ref().and_then(HeadState::branch_label);
        self.watch_targets = watch_targets_for_context(context, head_state.as_ref());
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
