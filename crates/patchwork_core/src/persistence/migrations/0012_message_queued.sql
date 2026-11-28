-- A message held in the queue when the app closed. The agent never read it.
ALTER TABLE messages ADD COLUMN queued INTEGER NOT NULL DEFAULT 0;
