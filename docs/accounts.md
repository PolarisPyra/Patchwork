# Accounts

How the hosted service in [`crates/api`](../crates/api) signs people in, what it stores, and how to
run and deploy it. The desktop app signs in to it from Settings → Account
([`crates/server/src/accounts.rs`](../crates/server/src/accounts.rs)); set `PATCHWORK_API_URL` when
starting `pnpm tauri dev` to point it at a local one.

## Shape

One binary, `patchwork-api`, does what Bitwarden splits into Identity, Api and Admin:

| Command | What it does |
| --- | --- |
| `patchwork-api serve` | serves the HTTP API, and prunes used codes and dead sessions every hour |
| `patchwork-api migrate` | brings the database up to date with [`crates/api/migrations`](../crates/api/migrations) |
| `patchwork-api user EMAIL` | prints the account and the devices signed in on it |
| `patchwork-api sign-out EMAIL` | ends every session of the account |

There is no admin site: looking after an account is done from the server's shell.

| Module | Holds |
| --- | --- |
| [`src/auth/`](../crates/api/src/auth) | passwords (`passwords.rs`), email codes (`codes.rs`), sessions and the `Session` extractor (`sessions.rs`), the sign-in routes (`ops.rs`) |
| [`src/accounts.rs`](../crates/api/src/accounts.rs) | the account: read, rename, change email or password, sign out everywhere, delete |
| [`src/devices.rs`](../crates/api/src/devices.rs) | the installs signed in to an account |
| [`src/events.rs`](../crates/api/src/events.rs) | the account's history: sign-ins, sign-outs, changes |
| [`src/http.rs`](../crates/api/src/http.rs) | the routes, the error shape, the JSON and client IP extractors |
| [`src/config.rs`](../crates/api/src/config.rs), [`src/email.rs`](../crates/api/src/email.rs) | settings from the environment, and sending email |

## Signing in

An email and password, then a six-digit code sent to that email:

1. **Register.** `POST /v1/auth/register` emails a code; `POST /v1/auth/register/finish` takes it
   with the password and signs the install in. The code proves the address, so the account is
   never made for an address its owner doesn't read.
2. **Sign in.** `POST /v1/auth/login` checks the password and emails a code, answering
   `{"status": "codeSent", "challenge"}`. `POST /v1/auth/login/verify` takes the challenge and the
   code and starts the session. With `"remember": true` it also returns a `rememberToken`, and a
   later login from the same install that sends it skips the code for 30 days, answering
   `{"status": "signedIn", …}` at once.
3. **Forgotten password.** `POST /v1/auth/password-reset` emails a code;
   `POST /v1/auth/password-reset/finish` takes it with the new password and ends every session.

How each part is kept safe:

- **Passwords** are hashed with Argon2id at OWASP's parameters (19 MiB, 2 passes, 1 lane) and a
  random salt each, stored as a PHC string (`$argon2id$v=19$m=19456,t=2,p=1$…`), so the
  parameters can be raised later without breaking old hashes. They need 12 to 128 characters,
  with no other rules. At most 4 hashes run at once, so a flood of logins can't use up memory.
  Unlike Bitwarden, the server sees the password: there is no vault encrypted with it, so the
  client-side key derivation Bitwarden needs for that has nothing to protect here.
- **Wrong passwords** answer `invalid_credentials` whether or not the address has an account,
  and take as long either way (an unknown address is checked against a stand-in hash). Ten wrong
  passwords for one address, or fifty from one IP, in 15 minutes answer 429 until the window
  passes, even with the right password.
- **Codes** are good for 10 minutes and 5 tries, and are used up once they match. The database
  keeps only an HMAC of each, keyed by `PATCHWORK_API_CODE_KEY`, so a copy of the database
  can't check a code. An address can be sent 5 emails an hour and 1 every 30 seconds; one IP can
  ask for 30 an hour.
- **Registering and resetting** answer 202 whether or not the address has an account. Registering
  an address that has one emails it a note saying so, instead of a code.
- **Sessions** are `pws_` tokens and remember tokens are `pwr_` tokens. The database keeps only
  the SHA-256 of each. A session lasts 90 days from its last use.
- **The security stamp** on each account is copied into its sessions. Changing it (signing out
  everywhere, a new password, a new email) ends every session that holds the old one, forgets
  every remembered install, and voids sign-in codes already sent.
- Changing the email or password and deleting the account each ask for the password again.

Email is the only second factor so far. Whoever reads the account's email can also reset its
password, so an authenticator app (TOTP) is the next one to add.

## Routes

The `/v1/auth/*` routes, except `logout`, and `health` need no token; every other route needs
`Authorization: Bearer <token>`. An error is `{"error": {"code", "message"}}`, where `message`
can be shown to the user as is. `device` is `{identifier, name, platform, version?}`.

| Route | Body | Answer |
| --- | --- | --- |
| `GET /v1/health` | | `{"ok": true}` when the database answers |
| `POST /v1/auth/register` | `{email}` | 202 |
| `POST /v1/auth/register/finish` | `{email, code, password, name?, device}` | `{token, account, device}` |
| `POST /v1/auth/login` | `{email, password, device, rememberToken?}` | `{status: "codeSent", challenge}` or `{status: "signedIn", token, account, device}` |
| `POST /v1/auth/login/verify` | `{challenge, code, device, remember?}` | `{token, account, device, rememberToken?}` |
| `POST /v1/auth/password-reset` | `{email}` | 202 |
| `POST /v1/auth/password-reset/finish` | `{email, code, password}` | 204; every session ends |
| `POST /v1/auth/logout` | | 204 |
| `GET /v1/accounts/me` | | `{id, email, name, createdAt}` |
| `PATCH /v1/accounts/me` | `{name}` | the account |
| `POST /v1/accounts/email` | `{newEmail, password}` | 202, and a code to the new address |
| `POST /v1/accounts/email/verify` | `{code}` | the account; other sessions end, the old address is told |
| `POST /v1/accounts/password` | `{currentPassword, newPassword}` | 204; other sessions end, the address is told |
| `POST /v1/accounts/security-stamp` | | 204; every other session ends |
| `DELETE /v1/accounts/me` | `{password}` | 204; the account, its devices and sessions are gone |
| `GET /v1/devices` | | `[{id, name, platform, version, lastSeenAt, current}]` |
| `DELETE /v1/devices/{id}` | | 204; the device's sessions end |
| `POST /v1/license/trial` | | `{key}`: a 14-day Basic license key for the account's email; 409 `trial_used` once the account or its email has had one |

`platform` is `linux`, `macos`, or `windows`. `identifier` is chosen by the install and kept, so
signing in again on the same install reuses its device.

## Data

Postgres, with the schema in [`migrations/0001_accounts.sql`](../crates/api/migrations/0001_accounts.sql):
`users`, `devices`, `sessions`, `email_codes`, `login_failures`, and `events`, and in
[`migrations/0002_trials.sql`](../crates/api/migrations/0002_trials.sql) `trials`, kept by
account and by email apart from `users`, so a deleted or moved account gets no second trial.
Emails are `citext`, so case never makes two accounts. Deleting a user deletes its devices,
sessions and the codes made for it, and keeps its events without the link to it.

A schema change is a new file in `migrations/`, numbered after the last. Never edit one that has
shipped: `migrate` refuses a database whose applied migrations differ from the files.

## Settings

| Variable | Default | |
| --- | --- | --- |
| `DATABASE_URL` | `postgresql:///patchwork?host=/var/run/postgresql&user=patchwork` | the local socket, with peer auth, as the `patchwork` role |
| `PATCHWORK_API_LISTEN` | `127.0.0.1:8080` | |
| `PATCHWORK_API_CODE_KEY` | required | 32 bytes in base64: `openssl rand -base64 32`. Changing it voids codes in flight, not sessions |
| `PATCHWORK_API_MAIL` | required | `smtp`, or `log` to write emails to the log instead, for development |
| `PATCHWORK_API_SMTP_URL` | required for `smtp` | `scheme://user:password@host:port/ehlo-name?tls=…`; see [Mail through Google Workspace](#mail-through-google-workspace) |
| `PATCHWORK_API_MAIL_FROM` | required for `smtp` | such as `Patchwork <accounts@patchworkeditor.com>` |
| `PATCHWORK_API_LICENSE_KEY_FILE` | unset, so trials answer 503 | the license signing key, `signing-key.pem` from `docs/licensing.md`; readable only by the service |
| `RUST_LOG` | `info` | |

The client IP comes from Cloudflare's `CF-Connecting-IP`. That is safe only because the firewall
lets nothing but Cloudflare reach port 443.

## Running it locally

With a Postgres you can reach (a throwaway one: `initdb -D <dir>`, then
`pg_ctl -D <dir> -o "-k <dir> -c listen_addresses=''" start` and `createdb -h <dir> patchwork`):

```sh
export DATABASE_URL="postgresql:///patchwork?host=<dir>"
export PATCHWORK_API_MAIL=log PATCHWORK_API_CODE_KEY=$(openssl rand -base64 32)
cargo run -p patchwork-api -- migrate
cargo run -p patchwork-api -- serve
```

Every code shows up in the log.

## Deploying

The server runs Debian 13, whose glibc is older than a rolling-release machine's, so build a static
binary for musl. It carries its own root certificates, so it needs nothing from the server's system:

```sh
rustup target add x86_64-unknown-linux-musl
CC_x86_64_unknown_linux_musl=gcc cargo build --release -p patchwork-api --target x86_64-unknown-linux-musl
```

`CC_…=gcc` lets the system C compiler build `ring` when no `musl-gcc` is installed. The binary
lives at `/opt/patchwork-api/patchwork-api`, a folder the `deploy` user owns, with
`/usr/local/bin/patchwork-api` linked to it. After the first time, the
[Deploy API workflow](#deploying-from-github) puts it there.

`/etc/patchwork-api.env`, owned by root and mode 600:

```sh
PATCHWORK_API_CODE_KEY=...
PATCHWORK_API_MAIL=smtp
PATCHWORK_API_SMTP_URL=smtp://smtp-relay.gmail.com:587/api.patchworkeditor.com?tls=required
PATCHWORK_API_MAIL_FROM=Patchwork <accounts@patchworkeditor.com>
PATCHWORK_API_LICENSE_KEY_FILE=/etc/patchwork-api/license-signing.pem
```

The license signing key is the one in `.license-signing/signing-key.pem`. Copy it to the server
with `install -d -m 700 -o patchwork /etc/patchwork-api` and
`install -m 400 -o patchwork signing-key.pem /etc/patchwork-api/license-signing.pem`, so only the
service can read it. Without it, free trials answer 503 and everything else works.

`/etc/systemd/system/patchwork-api.service`, run as the `patchwork` user so peer auth maps it to
the `patchwork` role:

```ini
[Unit]
Description=Patchwork API
After=network.target postgresql.service
Requires=postgresql.service

[Service]
User=patchwork
EnvironmentFile=/etc/patchwork-api.env
ExecStartPre=/usr/local/bin/patchwork-api migrate
ExecStart=/usr/local/bin/patchwork-api serve
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Then `systemctl enable --now patchwork-api`, and in the Caddyfile's `api.patchworkeditor.com`
block, replace the placeholder response with `reverse_proxy 127.0.0.1:8080`. Check it with
`curl https://api.patchworkeditor.com/v1/health`.

### Deploying from GitHub

[`.github/workflows/deploy-api.yml`](../.github/workflows/deploy-api.yml) runs after each push to
`main` that passes CI. When `crates/api` or `Cargo.lock` changed since the commit in
`/opt/patchwork-api/REVISION`, it builds the binary, copies it in as `deploy`, restarts the
service, and writes the new commit to `REVISION` once `/v1/health` answers. If it doesn't answer,
the previous binary (`patchwork-api.prev`) goes back. That can't undo a migration, and the old
binary won't start on a database that has one it doesn't know, so the run fails either way.

`deploy` can write only `/opt/patchwork-api` and run only `systemctl restart patchwork-api` as
root. The service still runs as `patchwork`, so the deploy key can change what that user runs, but
not take root. Its key may run commands but not forward ports or open a terminal.

The workflow reads three secrets of the `production` environment (Settings → Environments), which
can also be set to wait for an approval:

| Secret | |
| --- | --- |
| `API_DEPLOY_HOST` | the server's IP. It isn't in the repo, since Cloudflare hides it |
| `API_DEPLOY_SSH_KEY` | the private half of the `deploy` user's key |
| `API_DEPLOY_KNOWN_HOSTS` | the server's host key line, so the runner can't be sent elsewhere |

## Mail through Google Workspace

Google's SMTP relay sends as any address in the Workspace's domains and can trust the server by
its IP, so the server keeps no mail password.

1. In the Admin console, **Apps → Google Workspace → Gmail → Routing → SMTP relay service →
   Configure**: allowed senders **Only addresses in my domains**, authentication **Only accept mail
   from the specified IP addresses** with the server's IP, and **Require TLS encryption** on.
2. Make the sending address an alias or group (`accounts@patchworkeditor.com`), so it needs no
   seat of its own.
3. Set `PATCHWORK_API_SMTP_URL=smtp://smtp-relay.gmail.com:587/api.patchworkeditor.com?tls=required`.
   The path is the name the server introduces itself with (EHLO); without one it says `localhost`,
   which the relay can turn away.
4. In DNS for the sending domain: SPF `v=spf1 include:_spf.google.com ~all` (merged into any SPF
   record already there, since a domain may have only one), the DKIM key from **Apps → Google
   Workspace → Gmail → Authenticate email** (`google._domainkey`, then **Start authentication**),
   and DMARC at `_dmarc`, starting with `v=DMARC1; p=none; rua=mailto:<you>`.
5. On the server, `nc -vz smtp-relay.gmail.com 587` should connect.

Without the relay, a Workspace user with 2-Step Verification can send through
`smtps://<user>:<app password>@smtp.gmail.com:465` instead, from that user's own address, up to
2,000 messages a day.
