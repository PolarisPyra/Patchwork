-- When a reply finished, so the timeline can say how long it worked.
ALTER TABLE messages ADD COLUMN finished_at INTEGER;
