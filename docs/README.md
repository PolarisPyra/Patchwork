# Patchwork docs

Notes for people building, changing, and shipping Patchwork. The [main README](../README.md) is
for people using it.

| Guide | What it covers |
| --- | --- |
| [Development](development.md) | Requirements, running the app, building, tests, and other checks |
| [Architecture](architecture.md) | Repo layout, the SQLite data model, and where tasks and chats run |
| [Agent CLIs](agent-clis.md) | How each agent is detected, run, approved, steered, and imported |
| [Releasing](releasing.md) | git-cliff changelog, version tags, and the release workflow |
| [UI checks](../scripts/qa/README.md) | Browser measurements and screenshots of the running dev session |

## Adding a guide

Write a Markdown file in this folder with a short title as the first line (`# Title`), then link
it from the table above. Use the same tone as the existing guides: say what the page covers in
the first paragraph, then go straight to the commands, tables, and steps.

Keep a guide to one topic. If a page needs to explain how a feature works and how to change it,
split it into two pages and link them.

When a guide describes a command, file, or path, check that it still exists before you commit.
Link to code with repo-relative paths, such as `apps/server/src/orchestration/ingest.rs`, so the links keep
working in the GitHub viewer.
