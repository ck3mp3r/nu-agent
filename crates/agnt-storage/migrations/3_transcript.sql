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
);
