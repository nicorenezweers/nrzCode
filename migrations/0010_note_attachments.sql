-- Bilder in Notizen: UUIDs der Anhänge (JSON-Liste, Tabelle attachments) in ihrer Reihenfolge
ALTER TABLE prompt_notes ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]';
