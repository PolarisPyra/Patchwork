-- When a bot was archived; NULL while it is in the sidebar.
ALTER TABLE bots ADD COLUMN archived_at INTEGER;
