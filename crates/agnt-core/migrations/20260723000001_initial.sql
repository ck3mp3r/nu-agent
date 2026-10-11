CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT ''
);

CREATE TABLE entries (
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    entry_type TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (session_id, seq),
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);
