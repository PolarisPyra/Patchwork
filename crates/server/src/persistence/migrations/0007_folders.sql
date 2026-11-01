-- Folders inside a project. A thread names its folder, or none for the top level.
CREATE TABLE folders (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    position INTEGER NOT NULL
);
ALTER TABLE threads ADD COLUMN folder_id TEXT;
