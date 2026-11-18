-- A bot's own chat background, as JSON; NULL shows the one every chat shares.
ALTER TABLE bots ADD COLUMN background TEXT;
