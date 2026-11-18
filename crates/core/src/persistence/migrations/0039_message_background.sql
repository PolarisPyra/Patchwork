-- A reply's background tasks as JSON, so a task cut off by Patchwork closing shows as stopped
-- instead of vanishing; NULL for older messages.
ALTER TABLE messages ADD COLUMN background TEXT;
