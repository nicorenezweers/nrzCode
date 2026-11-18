-- Art der verbundenen Git-Plattform: 'nrzgit' (Vorgabe) oder 'lugit' (luGit: gleiche API für
-- Repositories, Pull Requests und Git über HTTP, aber ohne MCP-Server und mit eigenen Routen zum
-- Anlegen von Repositories). Wird beim Verbinden und Prüfen erkannt.
ALTER TABLE nrzgit_connections ADD COLUMN kind TEXT NOT NULL DEFAULT 'nrzgit';
