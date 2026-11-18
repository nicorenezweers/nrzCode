-- nrzGit und luGit gleichzeitig: je Benutzer höchstens eine Verbindung je Plattform (bisher genau
-- eine). `has_mcp`: hat die Plattform einen MCP-Server (nrzGit immer, luGit ab 1.1.0 – wird beim
-- Verbinden und Prüfen erkannt). Projekte merken sich, von welcher Plattform sie stammen.
ALTER TABLE projects ADD COLUMN forge TEXT NOT NULL DEFAULT '';
UPDATE projects
SET forge = COALESCE((SELECT c.kind FROM nrzgit_connections c WHERE c.user_id = projects.user_id), 'nrzgit')
WHERE repo <> '';

CREATE TABLE nrzgit_connections_new (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 'nrzgit' oder 'lugit'
    kind TEXT NOT NULL DEFAULT 'nrzgit',
    url TEXT NOT NULL,
    token TEXT NOT NULL,
    username TEXT NOT NULL DEFAULT '',
    verified_at INTEGER,
    last_error TEXT NOT NULL DEFAULT '',
    public_url TEXT NOT NULL DEFAULT '',
    has_mcp INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, kind)
);
INSERT INTO nrzgit_connections_new (user_id, kind, url, token, username, verified_at, last_error, public_url, has_mcp, created_at, updated_at)
SELECT user_id, kind, url, token, username, verified_at, last_error, public_url, CASE WHEN kind = 'lugit' THEN 0 ELSE 1 END, created_at, updated_at
FROM nrzgit_connections;
DROP TABLE nrzgit_connections;
ALTER TABLE nrzgit_connections_new RENAME TO nrzgit_connections;
