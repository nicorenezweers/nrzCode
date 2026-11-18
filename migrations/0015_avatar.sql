-- Eigenes Profilbild: Datei unter <data>/avatars/<user_id>; avatar_at = Zeitpunkt (Version der
-- Adresse `…/avatar?v=`), 0 = kein eigenes Bild (Identicon).
ALTER TABLE users ADD COLUMN avatar_at INTEGER NOT NULL DEFAULT 0;
