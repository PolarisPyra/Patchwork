-- The vault's data key sealed a second time, under a key made from the recovery key the user
-- was shown when the password was set. Both are NULL for a vault without one.
ALTER TABLE vault ADD COLUMN recovery_nonce BLOB;
ALTER TABLE vault ADD COLUMN recovery_key BLOB;
