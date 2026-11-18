-- Nachrichten, die während eines Laufs gesendet werden, warten hier (JSON-Liste) und gehen nach
-- dem Lauf zusammen als nächste Nachricht an den Agenten
ALTER TABLE threads ADD COLUMN queued TEXT NOT NULL DEFAULT '[]';
