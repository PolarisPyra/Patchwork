-- The agent a folder holds the tasks of, for a folder that sorting tasks by provider made; NULL for
-- the user's own folders.
ALTER TABLE folders ADD COLUMN sorted_by TEXT;
