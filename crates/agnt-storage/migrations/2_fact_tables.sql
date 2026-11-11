CREATE TABLE turns (
    session_id TEXT NOT NULL,
    turn_seq INTEGER NOT NULL,
    submitted_millis INTEGER NOT NULL,
    settled_millis INTEGER NOT NULL,
    model TEXT NOT NULL,
    agent TEXT NOT NULL,
    PRIMARY KEY (session_id, turn_seq)
);

CREATE TABLE entry_turns (
    session_id TEXT NOT NULL,
    made INTEGER NOT NULL,
    entry_id INTEGER NOT NULL,
    turn_seq INTEGER NOT NULL,
    PRIMARY KEY (session_id, made, entry_id),
    FOREIGN KEY (session_id, turn_seq) REFERENCES turns(session_id, turn_seq)
);

CREATE TABLE tool_calls (
    session_id TEXT NOT NULL,
    call TEXT NOT NULL,
    turn_seq INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'cancelled')),
    settled_millis INTEGER NOT NULL,
    PRIMARY KEY (session_id, call),
    FOREIGN KEY (session_id, turn_seq) REFERENCES turns(session_id, turn_seq)
);
