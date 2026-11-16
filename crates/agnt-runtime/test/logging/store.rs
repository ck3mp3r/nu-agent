//! Tests for the instance identity, the store's file composition, its header
//! block, and its collection of old instance files.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::{LogIdentity, LogRole, LogStore, instance_header};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_logging_identity_names_the_instance_after_its_name() -> Result<()> {
    // -- Setup & Fixtures
    let pid = std::process::id();

    // -- Exec & Check: the role never reaches the file name
    for role in [LogRole::Tui, LogRole::A2a, LogRole::Acp, LogRole::Run] {
        assert_eq!(
            LogIdentity::new(role)
                .with_name("alpha".to_string())
                .file_name(),
            format!("agent-alpha-{pid}.log"),
            "{role:?}"
        );
        assert_eq!(
            LogIdentity::new(role).file_name(),
            format!("agent-{pid}.log"),
            "an unnamed instance is named by its pid"
        );
    }
    Ok(())
}

#[test]
fn test_logging_identity_keeps_a_name_inside_the_logs_directory() -> Result<()> {
    // -- Setup & Fixtures
    let pid = std::process::id();

    // -- Exec & Check: a name is a file name part, never a path
    assert_eq!(
        LogIdentity::new(LogRole::Tui)
            .with_name("a/b c".to_string())
            .file_name(),
        format!("agent-a-b-c-{pid}.log")
    );
    assert_eq!(
        LogIdentity::new(LogRole::Tui)
            .with_name(String::new())
            .file_name(),
        format!("agent-{pid}.log"),
        "an empty name leaves the pid alone"
    );
    Ok(())
}

#[test]
fn test_logging_identity_header_records_the_identity() -> Result<()> {
    // -- Setup & Fixtures
    let pid = std::process::id();
    let named = LogIdentity::new(LogRole::Acp).with_name("alpha".to_string());
    let unnamed = LogIdentity::new(LogRole::Run);

    // -- Exec
    let named_header = instance_header(&named);
    let unnamed_header = instance_header(&unnamed);

    // -- Check: the block opens the file and names the instance
    let lines: Vec<&str> = named_header.lines().collect();
    let pid_line = format!("pid: {pid}");
    assert_eq!(lines.first().copied(), Some("---- instance log ----"));
    assert_eq!(lines.get(1).copied(), Some("name: alpha"));
    assert_eq!(lines.get(2).copied(), Some(pid_line.as_str()));
    assert_eq!(lines.get(3).copied(), Some("role: acp"));
    let started = lines
        .get(4)
        .and_then(|line| line.strip_prefix("started: "))
        .ok_or("the header records the start time")?;
    let parsed = chrono::DateTime::parse_from_rfc3339(started)?;
    assert_eq!(parsed.offset().local_minus_utc(), 0, "{started}");
    assert_eq!(
        started.len(),
        "2006-01-02T15:04:05.000Z".len(),
        "millisecond precision: {started}"
    );
    assert_eq!(lines.len(), 5, "{named_header}");

    // -- Check: an unnamed run carries no name line
    assert!(!unnamed_header.contains("name: "), "{unnamed_header}");
    assert!(unnamed_header.contains("role: run"), "{unnamed_header}");
    Ok(())
}

#[test]
fn test_logging_store_composes_the_instance_file_in_its_directory() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let identity = LogIdentity::new(LogRole::Tui).with_name("alpha".to_string());

    // -- Exec
    let file = store.instance_file(&identity);

    // -- Check
    assert_eq!(file, dir.join(identity.file_name()));
    Ok(())
}

#[test]
fn test_logging_store_session_sink_appends_to_the_instance_file() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let identity = LogIdentity::new(LogRole::Tui).with_name("alpha".to_string());

    // -- Exec: a second run of the same instance appends to the same file
    let mut first = store.session_sink(&identity);
    first.write_all(b"first run\n")?;
    first.flush()?;
    drop(first);
    let mut second = store.session_sink(&identity);
    second.write_all(b"second run\n")?;
    second.flush()?;

    // -- Check: one file, both runs kept, nothing rotated
    assert_eq!(
        std::fs::read_to_string(store.instance_file(&identity))?,
        "first run\nsecond run\n"
    );
    assert_eq!(entries(&dir)?, vec![identity.file_name()]);
    Ok(())
}

#[test]
fn test_logging_store_keeps_a_running_instance_file_when_others_start() -> Result<()> {
    // -- Setup & Fixtures: one instance owns a file and one line
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let owner = LogIdentity::new(LogRole::Tui).with_name("alpha".to_string());
    let owner_store = LogStore::open(&dir);
    let mut owner_sink = owner_store.session_sink(&owner);
    owner_sink.write_all(b"owner evidence\n")?;
    owner_sink.flush()?;

    // -- Exec: three more instances start while the owner runs
    let mut others: Vec<String> = Vec::new();
    for name in ["beta", "gamma", "delta"] {
        let identity = LogIdentity::new(LogRole::Run).with_name(name.to_string());
        others.push(identity.file_name());
        let mut sink = LogStore::open(&dir).session_sink(&identity);
        sink.write_all(b"other line\n")?;
        sink.flush()?;
    }

    // -- Check: the owner's file is untouched, each other instance wrote its own
    assert_eq!(
        std::fs::read_to_string(owner_store.instance_file(&owner))?,
        "owner evidence\n"
    );
    others.sort();
    let mut expected = vec![owner.file_name()];
    expected.extend(others);
    expected.sort();
    assert_eq!(entries(&dir)?, expected);
    Ok(())
}

#[test]
fn test_logging_store_gc_keeps_the_twenty_newest_instance_files() -> Result<()> {
    // -- Setup & Fixtures: 25 old instances, each an hour apart at least
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let owner = LogIdentity::new(LogRole::Tui).with_name("owner".to_string());
    for index in 0..25 {
        let age = Duration::from_secs(7_200 + (24 - index));
        seed_instance(&dir, &format!("agent-instance-{index:02}.log"), age)?;
    }

    // -- Exec
    store.gc(&owner);

    // -- Check: the five oldest are gone, the twenty newest stay
    let names = names_by_age(&dir)?;
    assert_eq!(names.len(), 20, "{names:?}");
    assert_eq!(
        names.first().map(String::as_str),
        Some("agent-instance-24.log")
    );
    assert_eq!(
        names.last().map(String::as_str),
        Some("agent-instance-05.log")
    );
    Ok(())
}

#[test]
fn test_logging_store_gc_never_collects_the_owners_own_file() -> Result<()> {
    // -- Setup & Fixtures: the owner's file is the oldest of them all
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let owner = LogIdentity::new(LogRole::Tui).with_name("owner".to_string());
    let own = owner.file_name();
    seed_instance(&dir, &own, Duration::from_secs(90_000))?;
    for index in 0..25 {
        let age = Duration::from_secs(7_200 + (24 - index));
        seed_instance(&dir, &format!("agent-instance-{index:02}.log"), age)?;
    }

    // -- Exec
    store.gc(&owner);

    // -- Check: the owner keeps its file, twenty others stay, five go
    assert!(dir.join(&own).exists(), "the owner's file survives");
    assert_eq!(entries(&dir)?.len(), 21);
    Ok(())
}

#[test]
fn test_logging_store_gc_never_collects_a_recently_written_file() -> Result<()> {
    // -- Setup & Fixtures: one live instance wrote a minute ago
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let owner = LogIdentity::new(LogRole::Tui).with_name("owner".to_string());
    seed_instance(&dir, "agent-frozen.log", Duration::from_secs(60))?;
    for index in 0..25 {
        let age = Duration::from_secs(7_200 + (24 - index));
        seed_instance(&dir, &format!("agent-instance-{index:02}.log"), age)?;
    }

    // -- Exec
    store.gc(&owner);

    // -- Check: a frozen-but-live writer's trail survives the collection
    assert!(
        dir.join("agent-frozen.log").exists(),
        "a file written within the last hour is never collected"
    );
    assert_eq!(entries(&dir)?.len(), 21);
    Ok(())
}

#[test]
fn test_logging_store_gc_leaves_legacy_files_alone() -> Result<()> {
    // -- Setup & Fixtures: legacy names beside more instances than the store keeps
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let owner = LogIdentity::new(LogRole::Tui).with_name("owner".to_string());
    seed_instance(&dir, "tui-alpha.log", Duration::from_secs(90_000))?;
    seed_instance(&dir, "a2a-alpha.log", Duration::from_secs(90_000))?;
    for index in 0..25 {
        let age = Duration::from_secs(7_200 + (24 - index));
        seed_instance(&dir, &format!("agent-instance-{index:02}.log"), age)?;
    }

    // -- Exec
    store.gc(&owner);

    // -- Check: a legacy file is not an instance file, so the collection never sees it
    assert!(dir.join("tui-alpha.log").exists());
    assert!(dir.join("a2a-alpha.log").exists());
    assert_eq!(entries(&dir)?.len(), 22);
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_logging_store_gc_skips_a_file_it_cannot_remove() -> Result<()> {
    // -- Setup & Fixtures: a read-only logs directory refuses every removal
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let owner = LogIdentity::new(LogRole::Tui).with_name("owner".to_string());
    for index in 0..25 {
        let age = Duration::from_secs(7_200 + (24 - index));
        seed_instance(&dir, &format!("agent-instance-{index:02}.log"), age)?;
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))?;

    // -- Exec
    store.gc(&owner);

    // -- Check: no panic, and every file is still there
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))?;
    assert_eq!(entries(&dir)?.len(), 25);
    Ok(())
}

#[test]
fn test_logging_store_falls_back_to_a_temp_file() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    // A file where the log directory should be: creating the directory fails.
    let blocked = temp.path().join("blocked");
    std::fs::write(&blocked, "")?;
    let store = LogStore::open(blocked.join("logs"));
    let identity = LogIdentity::new(LogRole::Tui).with_name("alpha".to_string());

    // -- Exec
    let sink = store.session_sink(&identity);
    let fallback = sink.path();

    // -- Check: a per-process temp file instead of a failed run
    assert_ne!(fallback, store.instance_file(&identity));
    assert_eq!(fallback.parent(), Some(std::env::temp_dir().as_path()));
    let name = fallback
        .file_name()
        .ok_or("the fallback has a file name")?
        .to_string_lossy()
        .to_string();
    assert!(
        name.starts_with("nu-agent-log-") && name.ends_with(".log"),
        "{name}"
    );
    let notice = sink.notice().ok_or("the fallback names its reason")?;
    assert!(notice.contains("blocked"), "{notice}");
    let _ = std::fs::remove_file(&fallback);
    Ok(())
}

#[test]
fn test_logging_store_fallback_names_the_unresolved_ladder() -> Result<()> {
    // -- Setup & Fixtures
    let store = LogStore::fallback();
    let identity = LogIdentity::new(LogRole::A2a).with_name("alpha".to_string());

    // -- Exec
    let sink = store.session_sink(&identity);

    // -- Check: the last-resort store keeps its file in the temp directory
    assert_eq!(sink.path().parent(), Some(std::env::temp_dir().as_path()));
    let notice = sink.notice().ok_or("the fallback names its reason")?;
    assert!(notice.contains("state ladder"), "{notice}");
    let _ = std::fs::remove_file(sink.path());
    Ok(())
}

/// WHEN the process log installs, THEN the instance file SHALL open with the
/// identity header, SHALL keep one header for a second `start`, and SHALL still
/// carry it when the run's level silences the file.
///
/// One test drives the process-global log, so it runs alone.
#[test]
#[serial_test::serial]
fn test_logging_store_start_installs_the_instance_log_with_its_header() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("logs");
    let store = LogStore::open(&dir);
    let identity = LogIdentity::new(LogRole::Run).with_name("alpha".to_string());
    assert!(
        LogStore::instance_path().is_none(),
        "a process without an installed sink has no instance file"
    );

    // -- Exec: the first start installs the process log and its header
    let mut writer = store.start(&identity);
    writer.write_all(b"kept until quit\n")?;
    writer.flush()?;

    // -- Check: the installed sink names the instance file
    let file = store.instance_file(&identity);
    assert_eq!(LogStore::instance_path().as_deref(), Some(file.as_path()));

    // -- Check: the header opens the file, ahead of any log line
    let content = std::fs::read_to_string(&file)?;
    let lines: Vec<&str> = content.lines().collect();
    let pid_line = format!("pid: {}", std::process::id());
    let header = [
        "---- instance log ----",
        "name: alpha",
        pid_line.as_str(),
        "role: run",
    ];
    assert_eq!(
        lines.get(..header.len()).map(<[&str]>::to_vec),
        Some(header.to_vec()),
        "{content}"
    );
    let started = lines
        .get(header.len())
        .and_then(|line| line.strip_prefix("started: "))
        .ok_or("the header records the start time")?;
    chrono::DateTime::parse_from_rfc3339(started)?;
    assert_eq!(
        lines.get(header.len() + 1).copied(),
        Some("kept until quit"),
        "the log line follows the header"
    );

    // -- Exec: a second start reuses the process log
    let mut second = LogStore::open(&dir).start(&identity);
    second.write_all(b"second call\n")?;
    second.flush()?;

    // -- Check: the header is written once
    let content = std::fs::read_to_string(&file)?;
    assert_eq!(
        content.matches("---- instance log ----").count(),
        1,
        "{content}"
    );
    assert!(content.contains("second call"), "{content}");

    // -- Exec: the run is an `off` run — the file is silenced
    let before = std::fs::read_to_string(&file)?;
    LogStore::set_process_silenced(true);
    writer.write_all(b"warn while off\n")?;
    writer.flush()?;

    // -- Check: the silenced line keeps nothing in the file
    assert_eq!(std::fs::read_to_string(&file)?, before);

    // -- Exec: the quit dumps the ring
    LogStore::flush_process_ring();

    // -- Check: the header and the dump survive the silence
    let content = std::fs::read_to_string(&file)?;
    assert!(content.starts_with("---- instance log ----"), "{content}");
    assert!(content.contains("---- ring dump"), "{content}");
    assert!(content.contains("warn while off"), "{content}");
    Ok(())
}

// region:    --- Test Support

/// Seed one instance log file, its mtime set `age` before now.
fn seed_instance(dir: &Path, name: &str, age: Duration) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, format!("{name}\n"))?;
    let file = std::fs::OpenOptions::new().append(true).open(&path)?;
    file.set_modified(SystemTime::now() - age)?;
    Ok(path)
}

/// Every entry name in `dir`, sorted.
fn entries(dir: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().to_string()))
        .collect::<std::io::Result<Vec<String>>>()?;
    names.sort();
    Ok(names)
}

/// The entry names in `dir`, newest first.
fn names_by_age(dir: &Path) -> Result<Vec<String>> {
    let mut entries: Vec<(SystemTime, String)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let modified = entry.metadata()?.modified()?;
        entries.push((modified, name));
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    Ok(entries.into_iter().map(|(_, name)| name).collect())
}

// endregion: --- Test Support
