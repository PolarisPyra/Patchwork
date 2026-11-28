-- The pairings were saved under a key that called them users. The list moves as it is.
UPDATE app_state SET key = 'gateway.pairings'
WHERE key = 'gateway.users' AND NOT EXISTS (SELECT 1 FROM app_state WHERE key = 'gateway.pairings');
DELETE FROM app_state WHERE key = 'gateway.users';
