-- Scheduled tasks: a prompt that starts a task at a fixed time.
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    -- The schedule as JSON, in the shape `types::Schedule` serializes to.
    schedule TEXT NOT NULL,
    -- The project to run in; NULL starts a chat in a scratch folder.
    project_id TEXT,
    agent TEXT NOT NULL,
    model TEXT,
    access_mode TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at INTEGER,
    next_run_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS scheduled_tasks_next_run ON scheduled_tasks (enabled, next_run_at);
