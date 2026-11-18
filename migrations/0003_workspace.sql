-- Bereich workspace: Pull Requests der Threads und Indizes für Projekte.

-- Pull Requests, die nrzCode für einen Thread in nrzGit angelegt hat (neuester zuerst angezeigt)
CREATE TABLE workspace_pull_requests (
    id INTEGER PRIMARY KEY,
    thread_id INTEGER NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    -- owner/name in nrzGit
    repo TEXT NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    base TEXT NOT NULL DEFAULT '',
    head TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
CREATE INDEX workspace_pull_requests_thread ON workspace_pull_requests (thread_id, id);

-- Letzte Prüfung der nrzGit-Verbindung (Fehlermeldung, leer = in Ordnung)
ALTER TABLE nrzgit_connections ADD COLUMN last_error TEXT NOT NULL DEFAULT '';

CREATE INDEX projects_user ON projects (user_id, last_activity_at);
CREATE INDEX threads_project ON threads (project_id);
