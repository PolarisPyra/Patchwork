-- The tokens a reply used, as JSON; NULL for agents that do not report usage.
ALTER TABLE messages ADD COLUMN usage TEXT;
