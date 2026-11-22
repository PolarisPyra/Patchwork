-- Threads are called chats.
ALTER TABLE threads RENAME TO chats;
ALTER TABLE messages RENAME COLUMN thread_id TO chat_id;
ALTER TABLE scheduled_task_runs RENAME COLUMN thread_id TO chat_id;
ALTER TABLE bots RENAME COLUMN thread_id TO chat_id;
ALTER TABLE reply_costs RENAME COLUMN thread_id TO chat_id;
ALTER TABLE quilts RENAME COLUMN thread_id TO chat_id;
DROP INDEX threads_session;
CREATE INDEX chats_session ON chats (agent, session_id);
DROP INDEX messages_thread;
CREATE INDEX messages_chat ON messages (chat_id, position);
DROP INDEX threads_bot_id;
CREATE INDEX chats_bot_id ON chats (bot_id);
UPDATE app_state SET key = 'last_chat_id' WHERE key = 'last_thread_id';
UPDATE app_state SET key = 'chat-committed:' || substr(key, 18) WHERE key LIKE 'thread-committed:%';
UPDATE app_state SET key = 'commit-chat:' || substr(key, 15) WHERE key LIKE 'commit-thread:%';
