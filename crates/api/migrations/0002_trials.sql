-- The free trial each account gets once. Kept apart from `users`, and by email as well, so deleting
-- the account or moving it to another address doesn't bring a second one.

create table trials (
  user_id    uuid primary key,
  email      citext not null unique,
  -- The `id` of the license key it was signed as.
  license_id text not null,
  expires_at timestamptz not null,
  created_at timestamptz not null default now()
);
