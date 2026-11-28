-- Phones and other computers paired with this one, which reach its server with a token of their
-- own. Only a hash of the token is kept, so the database cannot be used to connect.
CREATE TABLE devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    -- The scopes the token carries, as a JSON array such as ["read","write"].
    scopes TEXT NOT NULL,
    -- The token's SHA-256, as lowercase hex.
    token_hash TEXT NOT NULL UNIQUE,
    -- The token's first characters, so a person can tell two devices apart.
    token_hint TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER
);

-- Codes a new device trades for a token, each usable once until it expires.
CREATE TABLE pair_codes (
    code TEXT PRIMARY KEY,
    scopes TEXT NOT NULL,
    name TEXT,
    expires_at INTEGER NOT NULL
);
