-- An imported OpenCode chat, and whether its transcript still has to reach the agent.
ALTER TABLE threads ADD COLUMN imported_from TEXT;
ALTER TABLE threads ADD COLUMN context_pending INTEGER NOT NULL DEFAULT 0;
