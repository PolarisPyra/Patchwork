-- Bots are called agents.
ALTER TABLE bots RENAME TO agents;
ALTER TABLE chats RENAME COLUMN bot_id TO agent_id;
ALTER TABLE scheduled_tasks RENAME COLUMN bot_id TO agent_id;
DROP INDEX chats_bot_id;
CREATE INDEX chats_agent_id ON chats (agent_id);
-- The page the window reopens at names an agent's page "agent".
UPDATE app_state SET value = json_set(value, '$.kind', 'agent')
 WHERE key = 'last_place' AND json_valid(value) AND json_extract(value, '$.kind') = 'bot';
