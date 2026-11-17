-- The sidebar order of threads, lower first. Until a thread is dragged it
-- sorts by `-created_at`, which keeps the newest first.
ALTER TABLE threads ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
UPDATE threads SET position = -created_at;
