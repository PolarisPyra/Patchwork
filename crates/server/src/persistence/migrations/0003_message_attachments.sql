-- Files, folders, and images attached to a message.
ALTER TABLE messages ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]';
