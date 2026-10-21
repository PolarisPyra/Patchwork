//! Schema migrations, applied in order. `PRAGMA user_version` records how many
//! have run. Append new migrations; never edit a released one.

use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // 1: initial schema.
    "
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
    ",
    // 2: per-thread access mode and per-message approvals.
    "
    ALTER TABLE threads ADD COLUMN access_mode TEXT NOT NULL DEFAULT 'full-access';
    ALTER TABLE messages ADD COLUMN approvals TEXT NOT NULL DEFAULT '[]';
    ",
    // 3: files, folders, and images attached to a message.
    "ALTER TABLE messages ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]';",
    // 4: a message sent while a turn was running, which steered that turn.
    "ALTER TABLE messages ADD COLUMN steer INTEGER NOT NULL DEFAULT 0;",
    // 5: the thread's `/goal`, as JSON.
    "ALTER TABLE threads ADD COLUMN goal TEXT;",
    // 6: the sidebar order of threads, lower first. Until a thread is dragged it
    // sorts by `-created_at`, which keeps the newest first.
    "
    ALTER TABLE threads ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
    UPDATE threads SET position = -created_at;
    ",
    // 7: folders inside a project. A thread names its folder, or none for the top level.
    "
    CREATE TABLE folders (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        name TEXT NOT NULL,
        position INTEGER NOT NULL
    );
    ALTER TABLE threads ADD COLUMN folder_id TEXT;
    ",
    // 8: small app state that is not settings, such as the thread last open.
    "CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    // 9: the thread's reasoning effort; NULL leaves the agent's default.
    "ALTER TABLE threads ADD COLUMN effort TEXT;",
    // 10: when a reply finished, so the timeline can say how long it worked.
    "ALTER TABLE messages ADD COLUMN finished_at INTEGER;",
    // 11: the tokens a reply used, as JSON; NULL for agents that do not report usage.
    "ALTER TABLE messages ADD COLUMN usage TEXT;",
    // 12: a message held in the queue when the app closed. The agent never read it.
    "ALTER TABLE messages ADD COLUMN queued INTEGER NOT NULL DEFAULT 0;",
    // 13: when a thread was archived; NULL keeps it in the task list.
    "ALTER TABLE threads ADD COLUMN archived_at INTEGER;",
    // 14: titles of threads that began with `/goal` name the objective, not the command.
    "UPDATE threads SET title = TRIM(SUBSTR(title, 7)) WHERE title GLOB '/goal *';",
];

/// Brings the schema up to date. Returns the version the database was at before.
pub fn run(conn: &mut Connection) -> rusqlite::Result<usize> {
    let from: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(from) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(from)
}
