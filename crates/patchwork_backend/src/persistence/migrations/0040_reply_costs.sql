-- The token counts and cost of each reply whose chat was deleted or that was undone, so the
-- Costs totals still count it.
CREATE TABLE reply_costs (
    message_id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    title TEXT NOT NULL,
    project TEXT,
    agent TEXT NOT NULL,
    model TEXT,
    at INTEGER NOT NULL,
    input INTEGER NOT NULL,
    cache_read INTEGER NOT NULL,
    cache_write INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cost_micros INTEGER
);
