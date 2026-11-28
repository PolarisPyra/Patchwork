-- The vault: the secrets Patchwork keeps (chat app tokens, API keys, header and MCP values,
-- terminal variables), each sealed under the vault's data key with its name bound in.
CREATE TABLE secrets (
    name  TEXT PRIMARY KEY,
    nonce BLOB NOT NULL,
    value BLOB NOT NULL
);

-- The vault's data key, sealed under a wrapping key that is kept outside the database: in the
-- system credential store ('keychain'), or in a file in the config folder ('file').
CREATE TABLE vault (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    method      TEXT NOT NULL,
    nonce       BLOB NOT NULL,
    wrapped_key BLOB NOT NULL
);
