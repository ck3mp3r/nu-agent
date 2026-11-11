CREATE TABLE logs (
    session_id TEXT NOT NULL,
    name TEXT NOT NULL,
    bytes BLOB NOT NULL,
    PRIMARY KEY (session_id, name)
);

CREATE TABLE files (
    session_id TEXT NOT NULL,
    name TEXT NOT NULL,
    bytes BLOB NOT NULL,
    PRIMARY KEY (session_id, name)
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    made_millis INTEGER NOT NULL
);
