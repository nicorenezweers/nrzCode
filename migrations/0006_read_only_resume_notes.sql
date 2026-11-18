-- Nur-Lesen-Threads: Fragen zur Codebasis, der Agent darf nichts ändern (kein eigener Worktree,
-- keine Schreibwerkzeuge, keine Befehle ausser lesenden git-Befehlen, keine MCP-Server)
ALTER TABLE threads ADD COLUMN read_only INTEGER NOT NULL DEFAULT 0;

-- Automatisch weitermachen nach einem Rate-Limit: Zeitpunkt (Unix-Sekunden, NULL = nichts geplant)
-- und Zahl der Versuche hintereinander (für längere Pausen, wenn das Limit noch gilt)
ALTER TABLE threads ADD COLUMN resume_at INTEGER;
ALTER TABLE threads ADD COLUMN resume_attempts INTEGER NOT NULL DEFAULT 0;
CREATE INDEX threads_resume ON threads (resume_at) WHERE resume_at IS NOT NULL;

-- Notizblock: vorbereitete Prompts und Ideen, die man später sendet (je Benutzer, optional
-- einem Projekt zugeordnet)
CREATE TABLE prompt_notes (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    project_id INTEGER REFERENCES projects (id) ON DELETE SET NULL,
    text TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX prompt_notes_user ON prompt_notes (user_id, updated_at);
