-- The project's files before and after the turn that wrote the reply, as JSON git tree ids;
-- NULL for replies without snapshots.
ALTER TABLE messages ADD COLUMN turn TEXT;
