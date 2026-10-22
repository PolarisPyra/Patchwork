-- A reply's steps in order, and whether a steer cut it short; the thread that started a
-- subagent's thread, as JSON; and plan mode.
ALTER TABLE messages ADD COLUMN parts TEXT NOT NULL DEFAULT '[]';
ALTER TABLE messages ADD COLUMN cut_off INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN parent TEXT;
ALTER TABLE threads ADD COLUMN plan_mode INTEGER NOT NULL DEFAULT 0;
