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

// region:    --- M4

/// Drops the M3 transcript table.
///
/// The M3 table keys every row by an id derived from that row's field set, and
/// M4 grows the field set. An M3 row's stored id therefore no longer names the
/// row it holds, and no SQL statement can re-derive it. The table is the fold's
/// output, not its input: the log it folds stays, so the next attach folds and
/// writes it again under the M4 vocabulary.
pub(crate) const M4_DROP_TRANSCRIPT: &str = "DROP TABLE transcript";

/// `transcript`: one row per transcript block, keyed by the identity the row
/// derives from its own content.
///
/// `turn_seq` is the turn that persisted the row: every kind carries it, so a
/// reload reads the row's causal anchor without a second table. `row_seq` is the
/// row's position in its session's transcript, and the unique edge makes one row
/// per position: a fold that re-reads a session lands each row once.
///
/// The optional columns are the parts a row kind may not carry — a system row
/// has no tool, a call nothing recorded has no status — so an absent part is
/// NULL and never an invented value. The media columns carry a block that is not
/// text whole: where its bytes are, and the facts a provider needs to replay it.
pub(crate) const SCHEMA_TRANSCRIPT_M4: &str = "\
CREATE TABLE transcript (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    row_seq INTEGER NOT NULL,
    turn_seq INTEGER,
    kind TEXT NOT NULL CHECK (kind IN ('user', 'assistant', 'system', 'tool', 'compaction', 'media', 'reasoning', 'opaque')),
    content TEXT NOT NULL,
    tool_call_id TEXT,
    tool_name TEXT,
    tool_executed_name TEXT,
    tool_arguments TEXT,
    tool_result TEXT,
    tool_success INTEGER CHECK (tool_success IN (0, 1)),
    tool_status TEXT CHECK (tool_status IN ('success', 'failed', 'cancelled')),
    stamp_millis INTEGER,
    stamp_model TEXT,
    stamp_agent TEXT,
    media_owner TEXT CHECK (media_owner IN ('user', 'assistant', 'tool_result')),
    media_kind TEXT CHECK (media_kind IN ('image', 'audio', 'video', 'document')),
    media_source_kind TEXT CHECK (media_source_kind IN ('url', 'base64', 'file_id', 'raw', 'unknown')),
    media_source TEXT,
    media_raw BLOB,
    media_text TEXT,
    media_type TEXT,
    media_detail TEXT,
    reasoning_redacted INTEGER CHECK (reasoning_redacted IN (0, 1)),
    opaque_item TEXT,
    opaque_replay INTEGER CHECK (opaque_replay IN (0, 1)),
    UNIQUE (session_id, row_seq)
)";

/// Migration 4: the row vocabulary every entry's block fits, and the turn
/// binding each row carries.
pub(crate) const M4: &[&str] = &[M4_DROP_TRANSCRIPT, SCHEMA_TRANSCRIPT_M4];

// endregion: --- M4
