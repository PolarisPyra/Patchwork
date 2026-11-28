-- Per-thread access mode and per-message approvals.
ALTER TABLE threads ADD COLUMN access_mode TEXT NOT NULL DEFAULT 'full-access';
ALTER TABLE messages ADD COLUMN approvals TEXT NOT NULL DEFAULT '[]';
