-- The first schema: settings, projects, threads, and their messages.
CREATE TABLE settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    json TEXT NOT NULL
);
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    position INTEGER NOT NULL
);
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    title TEXT NOT NULL,
    agent TEXT NOT NULL,
    model TEXT,
    session_id TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX threads_session ON threads (agent, session_id);
CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    status TEXT NOT NULL,
    tools TEXT NOT NULL,
    plan TEXT NOT NULL,
    files TEXT NOT NULL,
    error TEXT
);
CREATE INDEX messages_thread ON messages (thread_id, position);
