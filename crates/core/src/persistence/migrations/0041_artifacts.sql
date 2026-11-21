-- What agents made for the person to look at, such as a page, chart or diagram, and each version
-- of it. An artifact is named by its chat and the key the agent gave it, so the agent can change
-- it later by that key.
CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    key TEXT NOT NULL,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    language TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (thread_id, key)
);

CREATE TABLE artifact_versions (
    artifact_id TEXT NOT NULL,
    n INTEGER NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (artifact_id, n)
);
