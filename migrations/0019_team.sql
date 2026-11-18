-- Team: mehrere Leute (jeder mit eigenem nrzCode) arbeiten am selben Repository, ihre Agenten
-- sprechen miteinander. Der gemeinsame Stand liegt als team.json in refs/nrzcode/team des
-- Repositorys; hier steht je Projekt die letzte bekannte Fassung und was noch zu senden ist.
CREATE TABLE team_projects (
    project_id INTEGER PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0,
    -- Agenten antworten selbstständig auf Nachrichten, die direkt an sie gehen
    auto_reply INTEGER NOT NULL DEFAULT 0,
    -- Commit der Referenz beim letzten Abgleich (leer = es gab sie nicht)
    remote_sha TEXT NOT NULL DEFAULT '',
    -- Letzter zusammengeführter Stand (TeamDoc als JSON)
    doc TEXT NOT NULL DEFAULT '{}',
    -- Wann die eigenen Agenten zuletzt gemeldet wurden
    presence_at INTEGER NOT NULL DEFAULT 0,
    synced_at INTEGER NOT NULL DEFAULT 0,
    error TEXT NOT NULL DEFAULT '',
    -- Bis dahin häufig abgleichen (Oberfläche offen, ein Agent wartet auf Antwort)
    hot_until INTEGER NOT NULL DEFAULT 0
);

-- Eigene Nachrichten, die noch nicht im Repository stehen (TeamMessage als JSON)
CREATE TABLE team_outbox (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES team_projects (project_id) ON DELETE CASCADE,
    message TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX team_outbox_project ON team_outbox (project_id);

-- Was ein Thread (Agent) schon bekommen hat; thread_id 0 = der Mensch (Benachrichtigung gezeigt)
CREATE TABLE team_seen (
    project_id INTEGER NOT NULL REFERENCES team_projects (project_id) ON DELETE CASCADE,
    thread_id INTEGER NOT NULL,
    message_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, thread_id, message_id)
);
