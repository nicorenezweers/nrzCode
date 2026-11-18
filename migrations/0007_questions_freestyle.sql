-- FreeStyle: der Agent entscheidet alles selbst, ohne Rückfragen (je Thread)
ALTER TABLE threads ADD COLUMN freestyle INTEGER NOT NULL DEFAULT 0;
-- Nach einem Rate-Limit automatisch weitermachen (je Thread); Vorgabe je Benutzer
ALTER TABLE threads ADD COLUMN resume_auto INTEGER NOT NULL DEFAULT 1;
ALTER TABLE users ADD COLUMN auto_resume INTEGER NOT NULL DEFAULT 1;
