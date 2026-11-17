-- A message sent while a turn was running, which steered that turn.
ALTER TABLE messages ADD COLUMN steer INTEGER NOT NULL DEFAULT 0;
