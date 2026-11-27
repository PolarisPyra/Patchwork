-- The salt the vault's wrapping key is derived with from the user's password, when the vault
-- has one ('password'): the key itself is kept nowhere.
ALTER TABLE vault ADD COLUMN salt BLOB;
