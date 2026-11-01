-- The custom agent a chat stays in the role of, and whether its agent still has to be told of it
-- (or that it ended).
ALTER TABLE threads ADD COLUMN role TEXT;
ALTER TABLE threads ADD COLUMN role_pending INTEGER NOT NULL DEFAULT 0;
