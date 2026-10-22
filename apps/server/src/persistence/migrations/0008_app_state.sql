-- Small app state that is not settings, such as the thread last open.
CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
