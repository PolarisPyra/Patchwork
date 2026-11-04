# Licensing

Patchwork is open core. Almost all of it is free software under the
[GNU AGPL v3.0 or later](LICENSE_AGPL.txt). A few features that come with a paid plan are
source-available under the [Patchwork Commercial License](LICENSE_COMMERCIAL.txt). [LICENSE](LICENSE)
says which applies where.

## Which license covers a file?

Everything under [`commercial/`](commercial/) is under the Patchwork Commercial License. Everything
else is under the AGPL, unless the file's header says otherwise.

| Path | License |
|---|---|
| `apps/`, `crates/`, `docs/`, everything else | AGPL-3.0-or-later |
| `commercial/` | Patchwork Commercial License v1.0 |

## What is free, and what is paid?

| Feature | Free | Basic |
|---|---|---|
| Every agent, task, chat, canvas, and setting in the AGPL code | Yes | Yes |
| One background for all chats | Yes | Yes |
| A background of its own for each chat | | Yes |

Paid features are unlocked by a license key, entered in **Settings → License**. The key is checked
on your machine; Patchwork never sends it anywhere.

## What does the Patchwork Commercial License let me do?

You may read, copy, and change the code in `commercial/` to develop and test it, without paying.
Using it in production, which means for anything other than developing and testing it, needs a
paid subscription. You may not remove or work around the license key check.

The Patchwork Commercial License is not an open source license under the OSI definition. The AGPL
is.

## Can I use the AGPL code in a closed-source product?

Not under the AGPL, which asks that anything you distribute, or run as a network service, is
offered under the same license with its source. If that does not suit you, a commercial license for
the AGPL code is available; open an issue titled "Commercial license".

## Can I contribute?

Yes. See [CONTRIBUTING.md](CONTRIBUTING.md). Contributions to either part are accepted on terms that
let the project offer them under both licenses.

## Can I use the Patchwork name?

The licenses cover the code, not the Patchwork name or logo. A fork needs its own name.
