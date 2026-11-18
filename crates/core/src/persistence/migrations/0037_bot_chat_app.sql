-- The chat app whose messages a bot answers, as JSON; NULL for none.
ALTER TABLE bots ADD COLUMN chat_app TEXT;
