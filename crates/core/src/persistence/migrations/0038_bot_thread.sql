-- The id of a bot's own chat; NULL until its first message.
ALTER TABLE bots ADD COLUMN thread_id TEXT;
