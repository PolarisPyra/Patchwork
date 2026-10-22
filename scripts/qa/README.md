# UI checks

Browser measurements and screenshots of the running dev session at http://localhost:1420.
Playwright drives headless Chromium. These are checks, not tests: nothing here asserts
or runs under a test runner.

## Run

Start the dev session first (`pnpm tauri dev`). The commands below only connect to it.

    pnpm qa:stripe                          # tool-stripe icon centering
    pnpm qa:shot /tmp/workspace.png         # whole workspace
    pnpm qa:shot /tmp/stripe.png nav        # one element, by selector
    pnpm qa:frames                          # frame rate and main-thread load while work runs
    QA_URL=http://localhost:4173/ pnpm qa:frames   # a production build (vite preview)
    pnpm qa:profile                         # CPU profile of startup, a long thread, streaming, typing, scrolling
    QA_URL=http://localhost:4173/ pnpm qa:profile  # the same on a production build; QA_CALLERS=fn,fn shows who calls them

The first run needs Chromium: `pnpm exec playwright install chromium`.

## Limits

`lib/tauri-stub.js` replaces the Rust backend with default settings and empty lists. It
checks layout and rendering. It does not check saved data, agent turns, or the backend.

## Adding a check

Add a file under `checks/` that calls `openWorkspace()` from `lib/open.mjs`, prints its
result, and calls `browser.close()`. Add a `qa:` script in the root `package.json` if
it will be reused.
