-- The recent runs of each scheduled task: started, failed, or skipped because its time went by.
CREATE TABLE IF NOT EXISTS scheduled_task_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    -- The thread the run started; NULL when it did not start one.
    thread_id TEXT,
    -- `started`, `failed`, or `skipped`, as `types::RunOutcome` serializes.
    outcome TEXT NOT NULL,
    detail TEXT
);

CREATE INDEX IF NOT EXISTS scheduled_task_runs_by_task ON scheduled_task_runs (task_id, started_at DESC);
