-- What another agent made of the files the turn that wrote the reply changed, as JSON;
-- NULL for replies no reviewer looked at, and for older replies.
ALTER TABLE messages ADD COLUMN review TEXT;
