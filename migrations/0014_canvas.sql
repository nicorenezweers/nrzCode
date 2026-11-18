-- Canvas-Modus: eine Fläche je Benutzer und Arbeitsbereich (project_key = Projekt-ID, 0 = ohne
-- Projekt). `layout` (Fenster, geschlossene Threads) gehört der Oberfläche, `shapes` (Zeichenebene)
-- teilen sich Oberfläche und Agenten (MCP: canvas_read, canvas_draw). Beides als JSON.
CREATE TABLE canvas_boards (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    project_key INTEGER NOT NULL DEFAULT 0,
    layout TEXT NOT NULL DEFAULT '{}',
    shapes TEXT NOT NULL DEFAULT '[]',
    rev INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, project_key)
);
