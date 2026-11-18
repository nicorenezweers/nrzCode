-- Adresse von nrzGit für Links im Browser. Die Verbindungsadresse (url) ist oft intern,
-- z. B. http://nrz-git_app_1:8380 auf Umbrel – die kennt der Browser nicht.
ALTER TABLE nrzgit_connections ADD COLUMN public_url TEXT NOT NULL DEFAULT '';
