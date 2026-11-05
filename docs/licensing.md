# Licensing and plans

How Patchwork's open core is split, how a plan unlocks a paid feature, and how to issue, check, and
rotate license keys. Which license covers what, for users, is in
[LICENSE_FAQ.md](../LICENSE_FAQ.md).

## The split

| Path | License | Holds |
| --- | --- | --- |
| everything outside `commercial/` | [AGPL-3.0-or-later](../LICENSE_AGPL.txt) | the app, and the plan check in [`crates/server/src/license.rs`](../crates/server/src/license.rs) |
| [`commercial/crates/licensing`](../commercial/crates/licensing) | [Patchwork Commercial License](../LICENSE_COMMERCIAL.txt) | the plans, what each unlocks, and the key check |
| [`commercial/web`](../commercial/web) | Patchwork Commercial License | the UI of paid features, as the `@patchwork/commercial-web` package |

The core may import from `commercial/`; nothing in `commercial/` is copied into the core. A paid
feature keeps its free part in the core (one background for every chat) and its paid part in
`commercial/` (a background for each chat).

## Plans

| Plan | Unlocks |
| --- | --- |
| Free | everything in the core |
| Basic | `Feature::ChatBackgrounds`: a background for each chat |

`Plan::features` in [`commercial/crates/licensing/src/lib.rs`](../commercial/crates/licensing/src/lib.rs)
is the one list. The server reads the plan from the stored key at each check
(`Server::allows`), and refuses a paid operation on a plan without it, so the UI is not the gate.
The UI reads the same answer through `useFeature` in
[`apps/web/src/hooks/useLicense.ts`](../apps/web/src/hooks/useLicense.ts) to lock the control.

### Adding a paid feature

1. Add a variant to `Feature` and to the plans that unlock it in `Plan::features`.
2. Check `self.allows(Feature::…)` in the server operation, and return an error that names the plan.
3. Put the feature's UI in `commercial/web/src/` and import it in the core as
   `@patchwork/commercial-web/<Component>`. Lock the control with `useFeature`.
4. Run `pnpm types`, so `apps/web/src/bindings/license.ts` lists the new feature.

## License keys

A key is `<payload>.<signature>`, both unpadded base64url. The payload is the JSON of `License`:

```json
{"id":"order-1042","plan":"basic","email":"buyer@example.com","expires":1825300800}
```

`expires` is in seconds since 1970. The signature is the payload's Ed25519 signature. Keys are
checked on the machine against `PUBLIC_KEY` in the licensing crate, so no server is asked. A key
whose `expires` has passed still shows in Settings → Account, as Expired, and gives the Free plan.

A key belongs to the account with its `email`. Settings → Account takes it only while signed in to
that account, and its plan holds only while this install is signed in to it; signed out, or signed
in to another account, the key stays saved, shows as Paused, and gives the Free plan. The check reads
the saved sign-in, so no server is asked here either.

### Issuing a key

The private key lives outside git, in `.license-signing/signing-key.pem` (ignored by
`.gitignore`), or better in a password manager. With it at hand:

```bash
b64() { base64 -w0 | tr '+/' '-_' | tr -d '='; }
printf '%s' '{"id":"order-1042","plan":"basic","email":"buyer@example.com","expires":1825300800}' > payload.json
openssl pkeyutl -sign -inkey .license-signing/signing-key.pem -rawin -in payload.json -out sig.bin
echo "$(b64 < payload.json).$(b64 < sig.bin)"
rm payload.json sig.bin
```

The buyer signs in with the key's email and pastes the printed key into Settings → Account.

### Rotating the signing key

Every key issued so far stops working when the public key changes, so rotate only when the private
key has leaked.

```bash
openssl genpkey -algorithm ed25519 -out .license-signing/signing-key.pem
openssl pkey -in .license-signing/signing-key.pem -pubout -outform DER | tail -c 32 | od -An -tu1
```

Put the 32 printed numbers in `PUBLIC_KEY`, release, and issue new keys to every buyer.
