-- Titles of threads that began with `/goal` name the objective, not the command.
UPDATE threads SET title = TRIM(SUBSTR(title, 7)) WHERE title GLOB '/goal *';
