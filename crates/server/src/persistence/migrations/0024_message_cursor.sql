-- Claude's id for the reply's last message, which undo and fork resume its session from;
-- NULL for other agents and older replies.
ALTER TABLE messages ADD COLUMN cursor TEXT;
