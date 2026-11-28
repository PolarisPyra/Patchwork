-- The chat app a person's message was sent from, as JSON such as "telegram"; NULL for messages
-- typed on the computer, for replies, and for older messages.
ALTER TABLE messages ADD COLUMN via TEXT;
