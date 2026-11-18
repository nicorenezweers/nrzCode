-- /btw: Seiten-Chat zum Zwischenstand eines Threads (eigener, kurzer Lauf eines Modells, der
-- den Agenten nicht unterbricht und nicht in den Verlauf des Threads kommt)
CREATE TABLE btw_entries (
    id INTEGER PRIMARY KEY,
    thread_id INTEGER NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    question TEXT NOT NULL,
    answer TEXT NOT NULL DEFAULT '',
    -- pending | done | error
    status TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX btw_entries_thread ON btw_entries (thread_id, id);
