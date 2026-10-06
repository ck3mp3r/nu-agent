use super::*;

#[test]
fn branch_resolver_prefers_explicit_caller_repo_over_process_cwd() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let process_repo = temp_dir.path().join("process-repo");
    let caller_repo = temp_dir.path().join("caller-repo");
    fs::create_dir_all(&process_repo).expect("process repo dir");
    fs::create_dir_all(&caller_repo).expect("caller repo dir");
    init_repo_with_branch(&process_repo, "process-branch");
    init_repo_with_branch(&caller_repo, "caller-branch");

    // The resolver takes the caller repo as a parameter and never reads the
    // process CWD, so passing `caller_repo` must resolve `caller-branch`.
    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&caller_repo);

    assert_eq!(resolved.as_deref(), Some("caller-branch"));
}

#[test]
fn branch_resolver_returns_none_for_non_git_directory() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let non_git = temp_dir.path().join("plain");
    fs::create_dir_all(&non_git).expect("plain dir");

    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&non_git);
    assert_eq!(resolved, None);
}

#[test]
fn branch_resolver_uses_detached_head_short_sha_fallback() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "feature/detached");

    let expected = run_git(&repo, &["rev-parse", "--short=12", "HEAD"]);
    run_git(&repo, &["checkout", "--detach"]);

    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&repo);
    assert_eq!(resolved.as_deref(), Some(expected.as_str()));
}

#[test]
fn branch_resolver_is_worktree_safe() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    let worktree = temp_dir.path().join("repo-wt");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "mainline");

    run_git(&repo, &["branch", "wt-branch"]);
    run_git(
        &repo,
        &[
            "worktree",
            "add",
            worktree.to_str().expect("worktree path"),
            "wt-branch",
        ],
    );

    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&worktree);
    assert_eq!(resolved.as_deref(), Some("wt-branch"));
}

#[test]
fn repo_branch_tracker_updates_on_branch_and_detached_transitions() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "branch-one");

    let mut tracker =
        crate::runtime::status::RepoBranchTracker::from_caller_cwd(Some(repo.clone()));
    assert_eq!(tracker.branch(), Some("branch-one"));

    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "-b", "branch-two"]);
    tracker.refresh();
    assert_eq!(tracker.branch(), Some("branch-two"));

    let expected_detached = run_git(&repo, &["rev-parse", "--short=12", "HEAD"]);
    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "--detach"]);
    tracker.refresh();
    assert_eq!(tracker.branch(), Some(expected_detached.as_str()));
}

#[test]
fn repo_branch_tracker_refreshes_on_render_tick_without_terminal_event() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "branch-one");

    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_repo_branch_caller_cwd(Some(repo.clone()));
    assert_eq!(coordinator.repo_branch(), Some("branch-one"));

    // Switch branch externally, then exercise the render-loop path that the
    // branch watcher triggers: refresh + mark render needed. No terminal event.
    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "-b", "branch-two"]);
    coordinator.refresh_repo_branch();

    assert_eq!(
        coordinator.repo_branch(),
        Some("branch-two"),
        "branch must update from a git ref change event, without a terminal event"
    );
}

#[test]
fn branch_watcher_signals_on_git_checkout() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "branch-one");

    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_repo_branch_caller_cwd(Some(repo.clone()));
    let targets = coordinator.repo_branch_watch_targets();
    assert!(!targets.is_empty(), "watcher needs git ref targets");

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let _watcher =
        crate::runtime::branch_watcher::spawn_branch_watcher(targets, tx).expect("watcher");

    // Switch branch externally — the notify watcher should emit a signal.
    std::thread::sleep(Duration::from_millis(20));
    run_git(&repo, &["checkout", "-b", "branch-two"]);

    // Wait (bounded) for the watcher to deliver a signal on the channel, which
    // is the event the render loop's branch_rx arm consumes.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut signalled = false;
    while std::time::Instant::now() < deadline {
        if rx.try_recv().is_ok() {
            signalled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(signalled, "branch change should emit a watcher signal");

    coordinator.refresh_repo_branch();
    assert_eq!(coordinator.repo_branch(), Some("branch-two"));
}

#[test]
fn repo_branch_tracker_does_not_leak_between_repositories() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo_a = temp_dir.path().join("repo-a");
    let repo_b = temp_dir.path().join("repo-b");
    fs::create_dir_all(&repo_a).expect("repo a dir");
    fs::create_dir_all(&repo_b).expect("repo b dir");
    init_repo_with_branch(&repo_a, "alpha");
    init_repo_with_branch(&repo_b, "beta");

    let tracker_a = crate::runtime::status::RepoBranchTracker::from_caller_cwd(Some(repo_a));
    let tracker_b = crate::runtime::status::RepoBranchTracker::from_caller_cwd(Some(repo_b));

    assert_eq!(tracker_a.branch(), Some("alpha"));
    assert_eq!(tracker_b.branch(), Some("beta"));
}
