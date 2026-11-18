-- Nutzung je Lauf, unabhängig von Threads und Projekten: Läufe werden mit ihrem Thread gelöscht,
-- ihr Verbrauch bleibt hier für die Nutzung (Limits, Kosten, Tokens) erhalten. Trigger halten die
-- Tabelle mit `runs` gleich; gelöschte Läufe verlieren nur den Bezug `run_id`. IDs von Threads und
-- Läufen können nach dem Löschen wieder vergeben werden – Sitzungen zählen darum über
-- `thread_key` („s<ID der ersten Zeile des Threads>“, AUTOINCREMENT vergibt keine ID doppelt).
-- Nebenläufe ohne eigenen Lauf (Seiten-Chat, Dieter) schreibt der Server direkt mit `run_id` NULL.
CREATE TABLE usage_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    thread_id INTEGER,
    thread_key TEXT,
    agent_kind TEXT NOT NULL,
    account_id INTEGER,
    model TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'running',
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0,
    started_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX usage_log_run ON usage_log (run_id);
CREATE INDEX usage_log_thread ON usage_log (thread_id);
CREATE INDEX usage_log_user ON usage_log (user_id, started_at);
CREATE INDEX usage_log_account ON usage_log (account_id, started_at);

INSERT INTO usage_log (run_id, user_id, thread_id, agent_kind, account_id, model, status, input_tokens, output_tokens,
                       cache_read_tokens, cache_write_tokens, reasoning_tokens, cost_usd, started_at, finished_at)
SELECT r.id, t.user_id, r.thread_id, r.agent_kind, r.account_id, r.model, r.status, r.input_tokens, r.output_tokens,
       r.cache_read_tokens, r.cache_write_tokens, r.reasoning_tokens, r.cost_usd, r.started_at, r.finished_at
FROM runs r JOIN threads t ON t.id = r.thread_id
ORDER BY r.id;
UPDATE usage_log SET thread_key = 's' || (SELECT MIN(u.id) FROM usage_log u WHERE u.thread_id = usage_log.thread_id);

-- Neuer Lauf: Schlüssel des Threads übernehmen (noch vorhandene Läufe), sonst einen neuen vergeben
CREATE TRIGGER usage_log_insert AFTER INSERT ON runs
BEGIN
    INSERT INTO usage_log (run_id, user_id, thread_id, thread_key, agent_kind, account_id, model, status, input_tokens,
                           output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, cost_usd, started_at, finished_at)
    SELECT NEW.id, t.user_id, NEW.thread_id,
           (SELECT u.thread_key FROM usage_log u WHERE u.thread_id = NEW.thread_id AND u.run_id IS NOT NULL LIMIT 1),
           NEW.agent_kind, NEW.account_id, NEW.model, NEW.status, NEW.input_tokens, NEW.output_tokens,
           NEW.cache_read_tokens, NEW.cache_write_tokens, NEW.reasoning_tokens, NEW.cost_usd, NEW.started_at, NEW.finished_at
    FROM threads t WHERE t.id = NEW.thread_id;
    UPDATE usage_log SET thread_key = 's' || id WHERE id = last_insert_rowid() AND thread_key IS NULL;
END;

CREATE TRIGGER usage_log_update AFTER UPDATE ON runs
BEGIN
    UPDATE usage_log SET
        agent_kind = NEW.agent_kind,
        account_id = NEW.account_id,
        model = NEW.model,
        status = NEW.status,
        input_tokens = NEW.input_tokens,
        output_tokens = NEW.output_tokens,
        cache_read_tokens = NEW.cache_read_tokens,
        cache_write_tokens = NEW.cache_write_tokens,
        reasoning_tokens = NEW.reasoning_tokens,
        cost_usd = NEW.cost_usd,
        started_at = NEW.started_at,
        finished_at = NEW.finished_at
    WHERE run_id = NEW.id;
END;

CREATE TRIGGER usage_log_delete AFTER DELETE ON runs
BEGIN
    UPDATE usage_log SET run_id = NULL WHERE run_id = OLD.id;
END;
