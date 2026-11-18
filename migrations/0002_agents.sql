-- nrzCode – Bereich agents: Anhänge, Skills, Ergänzungen an Konten, Läufen und Nachrichten.

-- Umgebungsvariable für den Schlüssel eines Kontos (OpenCode/Kilo: Anbieter wählbar,
-- z. B. ANTHROPIC_API_KEY oder OPENROUTER_API_KEY; leer = Vorgabe der Agent-Art)
ALTER TABLE agent_accounts ADD COLUMN secret_env TEXT NOT NULL DEFAULT '';
-- Vorgabe für die Denk-Stufe (low | medium | high | max, leer = Vorgabe des CLIs)
ALTER TABLE agent_accounts ADD COLUMN default_effort TEXT NOT NULL DEFAULT '';
CREATE INDEX agent_accounts_user ON agent_accounts (user_id, kind);

-- Gerendertes Markdown (user/assistant), beim Schreiben erzeugt
ALTER TABLE messages ADD COLUMN html TEXT NOT NULL DEFAULT '';

-- Denk-Stufe und Sitzungs-ID des Laufs (Läufe: queued | running | done | error | stopped)
ALTER TABLE runs ADD COLUMN effort TEXT NOT NULL DEFAULT '';
ALTER TABLE runs ADD COLUMN session_id TEXT NOT NULL DEFAULT '';
CREATE INDEX runs_status ON runs (status);

-- Angehängte Dateien (Bilder im Composer), Datei unter <data>/attachments/<uuid>
CREATE TABLE attachments (
    uuid TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    download_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX attachments_user ON attachments (user_id, created_at);

-- Skills: gespeicherte Prompt-Bausteine je Benutzer (im Composer mit `$`)
CREATE TABLE agent_skills (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    prompt TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX agent_skills_name ON agent_skills (user_id, name);
