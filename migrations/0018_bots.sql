-- Bot-Modus: dauerhafte KI-Teamkollegen mit eigenem Gedächtnis, eigenem Arbeitsordner
-- (<data>/bots/<id>) und Routinen. Ein Gespräch mit einem Bot ist ein Thread mit bot_id;
-- thread_id zeigt auf das aktuelle Gespräch.
CREATE TABLE bots (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    emoji TEXT NOT NULL DEFAULT '',
    -- Kurzbeschreibung unter dem Namen, z. B. „Recherche“
    role TEXT NOT NULL DEFAULT '',
    instructions TEXT NOT NULL DEFAULT '',
    -- Gedächtnis (Markdown), schreibt der Bot selbst über nrzcode remember/rewrite_memory
    memory TEXT NOT NULL DEFAULT '',
    agent_kind TEXT NOT NULL DEFAULT 'claude',
    account_id INTEGER REFERENCES agent_accounts (id) ON DELETE SET NULL,
    model TEXT NOT NULL DEFAULT '',
    effort TEXT NOT NULL DEFAULT '',
    thread_id INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX bots_user ON bots (user_id);

ALTER TABLE threads ADD COLUMN bot_id INTEGER;
CREATE INDEX threads_bot ON threads (bot_id);

-- Routinen: zu einer Uhrzeit an bestimmten Wochentagen einen Auftrag ins aktuelle Gespräch
-- schicken. time_min = Minuten nach Mitternacht in der Ortszeit des Benutzers,
-- tz_offset = Minuten östlich von UTC beim Speichern, days = Bitmaske Mo=1 … So=64.
CREATE TABLE bot_routines (
    id INTEGER PRIMARY KEY,
    bot_id INTEGER NOT NULL REFERENCES bots (id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    prompt TEXT NOT NULL,
    time_min INTEGER NOT NULL DEFAULT 480,
    days INTEGER NOT NULL DEFAULT 127,
    tz_offset INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX bot_routines_bot ON bot_routines (bot_id);
