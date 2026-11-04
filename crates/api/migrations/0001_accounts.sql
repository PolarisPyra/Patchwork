-- Accounts: who someone is, the installs they are signed in on, their sessions, the one-time codes
-- sent to their email, failed sign-ins, and what happened to the account.

create extension if not exists citext;

create table users (
  id                   uuid primary key,
  email                citext not null unique,
  email_verified_at    timestamptz,
  name                 text,
  -- Argon2id, as a PHC string: the parameters and salt travel with the hash.
  password_hash        text not null,
  -- Changing it ends every session made before, except the ones moved to the new stamp.
  security_stamp       uuid not null,
  last_login_at        timestamptz,
  last_email_change_at timestamptz,
  created_at           timestamptz not null default now(),
  updated_at           timestamptz not null default now()
);

-- One install of the app, by the random identifier it makes once.
create table devices (
  id               uuid primary key,
  user_id          uuid not null references users on delete cascade,
  identifier       text not null,
  name             text not null,
  platform         text not null,
  client_version   text,
  active           boolean not null default true,
  -- Set when a sign-in on this install chose to skip the email code for 30 days.
  remember_hash    bytea,
  remembered_until timestamptz,
  created_at       timestamptz not null default now(),
  last_seen_at     timestamptz,
  unique (user_id, identifier)
);

-- A signed-in session. Only the SHA-256 of its token is kept.
create table sessions (
  id             uuid primary key,
  user_id        uuid not null references users on delete cascade,
  device_id      uuid not null references devices on delete cascade,
  token_hash     bytea not null unique,
  security_stamp uuid not null,
  created_at     timestamptz not null default now(),
  last_used_at   timestamptz not null default now(),
  expires_at     timestamptz not null,
  revoked_at     timestamptz
);
create index sessions_user_id on sessions (user_id);

-- A one-time code sent by email. Only an HMAC of it is kept, and each allows a few attempts.
create type code_purpose as enum ('register', 'two_factor', 'reset_password', 'change_email');
create table email_codes (
  id          uuid primary key,
  email       citext not null,
  purpose     code_purpose not null,
  user_id     uuid references users on delete cascade,
  code_hash   bytea not null,
  attempts    int not null default 0,
  ip          inet,
  expires_at  timestamptz not null,
  consumed_at timestamptz,
  created_at  timestamptz not null default now()
);
create index email_codes_email on email_codes (email, purpose, created_at);
create index email_codes_ip on email_codes (ip, created_at);

-- A wrong password, counted per address and per IP to slow guessing.
create table login_failures (
  email      citext not null,
  ip         inet,
  created_at timestamptz not null default now()
);
create index login_failures_email on login_failures (email, created_at);
create index login_failures_ip on login_failures (ip, created_at);

-- The account's audit log.
create table events (
  id         bigserial primary key,
  user_id    uuid references users on delete set null,
  device_id  uuid references devices on delete set null,
  type       text not null,
  ip         inet,
  created_at timestamptz not null default now()
);
create index events_user_id on events (user_id, created_at);
