-- Zuletzt gezählte Änderungen je Thread (Zeilen hinzugefügt/entfernt) für die Anzeige +N −N
-- in der Seitenleiste und bei den Subagenten.
ALTER TABLE threads ADD COLUMN additions INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN deletions INTEGER NOT NULL DEFAULT 0;
