-- When a thread was archived; NULL keeps it in the task list.
ALTER TABLE threads ADD COLUMN archived_at INTEGER;
