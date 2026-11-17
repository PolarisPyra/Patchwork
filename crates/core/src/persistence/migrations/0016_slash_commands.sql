-- Provider-native slash commands discovered in the CLI initialization frame.
ALTER TABLE threads ADD COLUMN slash_commands TEXT NOT NULL DEFAULT '[]';
