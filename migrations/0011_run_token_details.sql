-- Genaue Statistik: Tokens aus dem Cache (gelesen/geschrieben) und Denk-Tokens je Lauf.
-- input_tokens enthält wie bisher alle Eingabe-Tokens (auch die aus dem Cache),
-- output_tokens alle Ausgabe-Tokens (auch die Denk-Tokens).
ALTER TABLE runs ADD COLUMN cache_read_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0;
