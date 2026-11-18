-- The tools a bot's runs may not use, as JSON; NULL blocks none.
ALTER TABLE bots ADD COLUMN blocked TEXT;
