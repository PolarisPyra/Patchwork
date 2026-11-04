# Patchwork commercial

The paid features of Patchwork. Everything in this folder is under the
[Patchwork Commercial License](../LICENSE_COMMERCIAL.txt), not the AGPL that covers the rest of the
repo. You may read, change, and run it to develop and test Patchwork; using it in production needs a
paid plan. [LICENSE_FAQ.md](../LICENSE_FAQ.md) explains what that means.

| Folder | Holds |
| --- | --- |
| [`crates/licensing`](crates/licensing) | The plans, what each unlocks, and the license key check (`patchwork-licensing`) |
| [`web`](web) | The UI of paid features (`@patchwork/commercial-web`) |

How a plan gates a feature, and how keys are issued, is in [Licensing and plans](../docs/licensing.md).
