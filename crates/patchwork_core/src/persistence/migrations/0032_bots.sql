-- Bots: a name, standing instructions, the agent they run on, and the projects they may work in.
-- Each run is a thread that names its bot.
CREATE TABLE bots (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    instructions TEXT NOT NULL DEFAULT '',
    agent TEXT NOT NULL,
    model TEXT,
    effort TEXT,
    access_mode TEXT NOT NULL,
    projects TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL
);
ALTER TABLE threads ADD COLUMN bot_id TEXT;
CREATE INDEX threads_bot_id ON threads (bot_id);
