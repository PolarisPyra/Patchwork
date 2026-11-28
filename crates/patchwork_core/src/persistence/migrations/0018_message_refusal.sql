-- Why a safety classifier blocked a reply, as JSON; NULL when nothing was blocked.
ALTER TABLE messages ADD COLUMN refusal TEXT;
