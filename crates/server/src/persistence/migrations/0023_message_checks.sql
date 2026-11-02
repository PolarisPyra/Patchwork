-- What the project's own check command made of the turn that wrote the reply, as JSON;
-- NULL for replies whose turn changed no files, and for older replies.
ALTER TABLE messages ADD COLUMN checks TEXT;
