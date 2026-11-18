-- nrzCode – Grundschema. Zeitangaben als Unix-Sekunden, Wahrheitswerte als 0/1.
-- Weitere Migrationen je Bereich: 0002_agents.sql, 0003_workspace.sql.

CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL,
    lower_name TEXT NOT NULL UNIQUE,
    full_name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0,
    is_active INTEGER NOT NULL DEFAULT 1,
    language TEXT NOT NULL DEFAULT '',
    theme TEXT NOT NULL DEFAULT 'dark',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_login_at INTEGER
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    user_agent TEXT NOT NULL DEFAULT ''
);
CREATE INDEX sessions_user ON sessions (user_id);

-- Einstellungen der Instanz (Schlüssel → Wert)
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Verbindung zu nrzGit je Benutzer: Adresse und Zugriffstoken (wird nie ausgeliefert)
CREATE TABLE nrzgit_connections (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    token TEXT NOT NULL,
    -- Benutzername in nrzGit (aus /api/v1/me)
    username TEXT NOT NULL DEFAULT '',
    verified_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Agent-Konten: ein Abo oder Schlüssel eines Anbieters. Jedes Konto hat ein eigenes
-- Home-Verzeichnis (<data>/accounts/<id>/home), damit mehrere Abos desselben Anbieters
-- (z. B. zwei Claude-Code-Konten) getrennt bleiben.
CREATE TABLE agent_accounts (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- claude | codex | gemini | opencode | kilo | grok
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    -- login (Anmeldung im CLI, im Home gespeichert) | api_key | oauth_token
    auth_kind TEXT NOT NULL DEFAULT 'login',
    -- API-Schlüssel oder OAuth-Token (nie ausgeliefert, nur Hinweis)
    secret TEXT NOT NULL DEFAULT '',
    default_model TEXT NOT NULL DEFAULT '',
    -- unknown | ok | login_required | error
    status TEXT NOT NULL DEFAULT 'unknown',
    status_message TEXT NOT NULL DEFAULT '',
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_used_at INTEGER
);

-- Projekte: nrzGit-Repositories, ausgecheckt unter <data>/workspaces/<id>/repo
CREATE TABLE projects (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    -- owner/name in nrzGit (leer bei lokalen Projekten)
    repo TEXT NOT NULL DEFAULT '',
    clone_url TEXT NOT NULL DEFAULT '',
    default_branch TEXT NOT NULL DEFAULT 'main',
    -- cloning | ready | error
    status TEXT NOT NULL DEFAULT 'cloning',
    status_message TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_activity_at INTEGER NOT NULL
);

-- Threads: Unterhaltung mit einem Agenten in einem Projekt (oder ohne Projekt).
-- Subagenten sind Threads mit parent_id.
CREATE TABLE threads (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    project_id INTEGER REFERENCES projects (id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES threads (id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    -- idle | working | waiting | done | error | settled
    status TEXT NOT NULL DEFAULT 'idle',
    agent_kind TEXT NOT NULL DEFAULT 'claude',
    account_id INTEGER REFERENCES agent_accounts (id) ON DELETE SET NULL,
    model TEXT NOT NULL DEFAULT '',
    effort TEXT NOT NULL DEFAULT '',
    -- Branch des Worktrees bzw. Hauptbranch
    branch TEXT NOT NULL DEFAULT '',
    -- Eigener Worktree (sonst direkt im Hauptcheckout des Projekts)
    use_worktree INTEGER NOT NULL DEFAULT 1,
    worktree_path TEXT NOT NULL DEFAULT '',
    -- Sitzungs-ID des Agenten zum Fortsetzen (claude --resume, codex resume …)
    provider_session_id TEXT NOT NULL DEFAULT '',
    unread INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_activity_at INTEGER NOT NULL
);
CREATE INDEX threads_user ON threads (user_id, last_activity_at);
CREATE INDEX threads_parent ON threads (parent_id);

-- Läufe: ein Aufruf des Agenten (eine Benutzernachricht → Antwort)
CREATE TABLE runs (
    id INTEGER PRIMARY KEY,
    thread_id INTEGER NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    agent_kind TEXT NOT NULL,
    account_id INTEGER REFERENCES agent_accounts (id) ON DELETE SET NULL,
    model TEXT NOT NULL DEFAULT '',
    -- running | done | error | stopped
    status TEXT NOT NULL DEFAULT 'running',
    exit_code INTEGER,
    error TEXT NOT NULL DEFAULT '',
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0,
    started_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX runs_thread ON runs (thread_id, started_at);

-- Nachrichten im Thread. role: user | assistant | tool | system | subagents | error
CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    thread_id INTEGER NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    run_id INTEGER REFERENCES runs (id) ON DELETE SET NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    data TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX messages_thread ON messages (thread_id, id);

-- Zugriffstokens für den nrzCode-MCP-Server (je Lauf erzeugt, mit dem Lauf ungültig)
CREATE TABLE run_tokens (
    token_hash TEXT PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    thread_id INTEGER NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);
