-- Seitenleiste: Threads ausblenden und von Hand sortieren (Ziehen und Ablegen).
-- sort_key leer = nach letzter Aktivität; gesetzt = feste Stelle (gleiche Skala wie die Zeit).
ALTER TABLE threads ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN sort_key INTEGER;
