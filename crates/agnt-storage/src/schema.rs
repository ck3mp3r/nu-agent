//! The SQL of every migration, oldest first.

// region:    --- M1

/// `logs`: rig's log transport, one opaque BLOB per log name.
pub(crate) const SCHEMA_LOGS: &str = "\
CREATE TABLE logs (
    session_id TEXT NOT NULL,
    name TEXT NOT NULL,
    bytes BLOB NOT NULL,
    PRIMARY KEY (session_id, name)
)";

/// `files`: rig's side files, one opaque BLOB per file name.
pub(crate) const SCHEMA_FILES: &str = "\
CREATE TABLE files (
    session_id TEXT NOT NULL,
    name TEXT NOT NULL,
    bytes BLOB NOT NULL,
    PRIMARY KEY (session_id, name)
)";

/// `sessions`: the session index, observed when a session first persists.
pub(crate) const SCHEMA_SESSIONS: &str = "\
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    made_millis INTEGER NOT NULL
)";

/// Migration 1: the baseline schema.
pub(crate) const M1: &[&str] = &[SCHEMA_LOGS, SCHEMA_FILES, SCHEMA_SESSIONS];

// endregion: --- M1

// region:    --- M2

/// `turns`: one row per nu-agent turn, every fact known when it settled.
pub(crate) const SCHEMA_TURNS: &str = "\
CREATE TABLE turns (
    session_id TEXT NOT NULL,
    turn_seq INTEGER NOT NULL,
    submitted_millis INTEGER NOT NULL,
    settled_millis INTEGER NOT NULL,
    model TEXT NOT NULL,
    agent TEXT NOT NULL,
    PRIMARY KEY (session_id, turn_seq)
)";

/// `entry_turns`: rig's per-entry `MessageKey`, attributed to the turn that
/// persisted it.
pub(crate) const SCHEMA_ENTRY_TURNS: &str = "\
CREATE TABLE entry_turns (
    session_id TEXT NOT NULL,
    made INTEGER NOT NULL,
    entry_id INTEGER NOT NULL,
    turn_seq INTEGER NOT NULL,
    PRIMARY KEY (session_id, made, entry_id),
    FOREIGN KEY (session_id, turn_seq) REFERENCES turns(session_id, turn_seq)
)";

/// `tool_calls`: one settled call's status, attributed to its turn.
pub(crate) const SCHEMA_TOOL_CALLS: &str = "\
CREATE TABLE tool_calls (
    session_id TEXT NOT NULL,
    call TEXT NOT NULL,
    turn_seq INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'cancelled')),
    settled_millis INTEGER NOT NULL,
    PRIMARY KEY (session_id, call),
    FOREIGN KEY (session_id, turn_seq) REFERENCES turns(session_id, turn_seq)
)";

/// Migration 2: the host's fact tables.
pub(crate) const M2: &[&str] = &[SCHEMA_TURNS, SCHEMA_ENTRY_TURNS, SCHEMA_TOOL_CALLS];

// endregion: --- M2

// region:    --- M3

/// `transcript`: one row per transcript block, keyed by the identity the row
/// derives from its own content.
///
/// `row_seq` is the row's position in its session's transcript, and the unique
/// edge makes one row per position: a fold that re-reads a session lands each
/// row once. The optional columns are the parts a row kind may not carry — a
/// system row has no tool, a call nothing recorded has no status — so an absent
/// part is NULL and never an invented value.
pub(crate) const SCHEMA_TRANSCRIPT: &str = "\
CREATE TABLE transcript (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    row_seq INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('user', 'assistant', 'system', 'tool', 'compaction')),
    content TEXT NOT NULL,
    tool_name TEXT,
    tool_arguments TEXT,
    tool_result TEXT,
    tool_success INTEGER CHECK (tool_success IN (0, 1)),
    tool_status TEXT CHECK (tool_status IN ('success', 'failed', 'cancelled')),
    stamp_millis INTEGER,
    stamp_model TEXT,
    stamp_agent TEXT,
    UNIQUE (session_id, row_seq)
)";

/// Migration 3: the transcript rows the store folds.
pub(crate) const M3: &[&str] = &[SCHEMA_TRANSCRIPT];

// endregion: --- M3
