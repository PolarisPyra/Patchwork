//! Architecture rules the compiler can't see: crate layering and source patterns that must not
//! come back.
//!
//! Both tests read the workspace's files, so `cargo test --workspace` checks every crate.

// Rust guideline compliant 2026-10-09

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The workspace root, two folders above this crate.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_toml(path: &Path) -> toml::Table {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    text.parse()
        .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

/// A crate's short name: `patchwork-backend` is `core`.
fn short(package: &str) -> &str {
    package.strip_prefix("patchwork-").unwrap_or(package)
}

/// The package names a manifest's dependency table refers to, following `package = "..."`.
fn table_deps(table: Option<&toml::Value>, into: &mut BTreeSet<String>) {
    let Some(table) = table.and_then(toml::Value::as_table) else {
        return;
    };
    for (key, spec) in table {
        let package = spec
            .get("package")
            .and_then(toml::Value::as_str)
            .unwrap_or(key);
        into.insert(package.to_owned());
    }
}

/// Every package a manifest names, in normal, dev and build dependencies and per target.
fn manifest_deps(manifest: &toml::Table) -> BTreeSet<String> {
    const TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut deps = BTreeSet::new();
    for name in TABLES {
        table_deps(manifest.get(name), &mut deps);
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            for name in TABLES {
                table_deps(target.get(name), &mut deps);
            }
        }
    }
    deps
}

/// The workspace graph: each member's short name and the members it depends on.
fn workspace_graph() -> BTreeMap<String, BTreeSet<String>> {
    let root = root();
    let workspace = read_toml(&root.join("Cargo.toml"));
    let members: Vec<&str> = workspace["workspace"]["members"]
        .as_array()
        .expect("workspace.members is a list")
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    let manifests: Vec<toml::Table> = members
        .iter()
        .map(|member| read_toml(&root.join(member).join("Cargo.toml")))
        .collect();
    let names: BTreeSet<String> = manifests
        .iter()
        .map(|m| {
            m["package"]["name"]
                .as_str()
                .expect("package.name")
                .to_owned()
        })
        .collect();
    manifests
        .iter()
        .map(|m| {
            let name = m["package"]["name"].as_str().expect("package.name");
            let inside = manifest_deps(m)
                .into_iter()
                .filter(|dep| names.contains(dep))
                .map(|dep| short(&dep).to_owned())
                .collect();
            (short(name).to_owned(), inside)
        })
        .collect()
}

/// Which workspace crates a crate may depend on.
enum Allow {
    /// Only these.
    Only(&'static [&'static str]),
    /// Anything but these.
    Never(&'static [&'static str]),
}

/// The layering. A crate that is on no line may depend on anything not forbidden by `BINARIES`.
const LAYERS: &[(&str, Allow, &str)] = &[
    (
        "themes",
        Allow::Only(&[]),
        "themes is a leaf: theme, scene and code-theme data with no workspace crate under it",
    ),
    (
        "rpc-protocol",
        Allow::Only(&[]),
        "protocol is a leaf: the wire types every transport shares",
    ),
    (
        "platform",
        Allow::Only(&[]),
        "system is the platform layer the backend is built on, not the other way round",
    ),
    (
        "dylib",
        Allow::Only(&[]),
        "dylib only bundles third-party crates for the dynamic debug build",
    ),
    (
        "oxidation-client",
        Allow::Only(&[]),
        "oxidation-client is a lightweight leaf SDK shared by the reloader and managed apps",
    ),
    (
        "vault",
        Allow::Only(&["platform"]),
        "vault seals secrets through system and knows nothing of core or providers",
    ),
    (
        "git",
        Allow::Only(&["platform"]),
        "git runs git through platform and knows nothing of backend or views",
    ),
    (
        "provider-core",
        Allow::Only(&["platform"]),
        "provider-core holds shared provider schema, discovery, and probing with no core dependency",
    ),
    (
        "provider-acp",
        Allow::Only(&["platform", "provider-core"]),
        "provider-acp defines ACP agent registries on top of provider-core and system",
    ),
    (
        "provider-api",
        Allow::Only(&["platform", "vault", "provider-core"]),
        "provider-api stores API providers and secrets without depending on core or other providers",
    ),
    (
        "provider-claude",
        Allow::Only(&["platform", "provider-core"]),
        "provider-claude isolates Claude CLI discovery and config on top of provider-core",
    ),
    (
        "provider-codex",
        Allow::Only(&["platform", "provider-core"]),
        "provider-codex isolates Codex CLI discovery and config on top of provider-core",
    ),
    (
        "provider-antigravity",
        Allow::Only(&["platform", "provider-core"]),
        "provider-antigravity isolates Antigravity CLI probing and login on top of provider-core",
    ),
    (
        "provider-opencode",
        Allow::Only(&["platform", "provider-core"]),
        "provider-opencode isolates OpenCode CLI discovery and server bridge on top of provider-core",
    ),
    (
        "daemon",
        Allow::Only(&["platform", "provider-core"]),
        "daemon handles updates, restarts, crashes, and logs without depending on backend or views",
    ),
    (
        "rpc-server",
        Allow::Only(&["rpc-protocol", "platform"]),
        "rpc-server serves the RPC protocol and pairing without depending on core or views",
    ),
    (
        "rpc-client",
        Allow::Only(&["rpc-protocol", "rpc-server", "platform"]),
        "client-runtime speaks the wire protocol to any Patchwork server without depending on core or views",
    ),
    (
        "ui-theme",
        Allow::Only(&["themes"]),
        "ui-theme is the GPUI token and asset foundation and must never reach into core or screens",
    ),
    (
        "ui-components",
        Allow::Only(&["themes", "ui-theme"]),
        "ui-components holds generic GPUI primitives and must never reach into core or screens",
    ),
    (
        "ui-backdrop",
        Allow::Only(&["themes", "ui-theme", "ui-components"]),
        "ui-backdrop renders ambient scenes and presets without reaching into core or screens",
    ),
    (
        "ui-markdown",
        Allow::Only(&["themes", "ui-theme", "ui-components"]),
        "ui-markdown renders rich text and syntax highlighting without reaching into core or screens",
    ),
    (
        "ui-editor",
        Allow::Only(&["themes", "ui-theme", "ui-components", "ui-markdown"]),
        "ui-editor is the reusable GPUI text area and must never reach into core or screens",
    ),
    (
        "backend",
        Allow::Never(&[
            "ui-theme",
            "ui-components",
            "ui-backdrop",
            "ui-markdown",
            "ui-editor",
            "desktop-shared",
            "desktop-browser",
            "desktop-android",
            "desktop-settings",
            "desktop-search",
            "desktop-onboarding",
            "desktop-quilts",
            "desktop-dialogs",
            "patchwork",
            "remote",
            "oxidation",
        ]),
        "backend is the headless backend and must not depend on a view or a binary",
    ),
    (
        "desktop-shared",
        Allow::Only(&["backend", "themes", "ui-theme", "ui-components"]),
        "desktop-shared binds core domain types to UI marks and sits below all desktop feature crates",
    ),
    (
        "desktop-browser",
        Allow::Never(&[
            "desktop-android",
            "desktop-settings",
            "desktop-search",
            "desktop-onboarding",
            "desktop-quilts",
            "desktop-dialogs",
        ]),
        "desktop-browser is a sidecar surface sitting below screens like quilts",
    ),
    (
        "desktop-onboarding",
        Allow::Never(&[
            "desktop-settings",
            "desktop-search",
            "desktop-quilts",
            "desktop-dialogs",
            "desktop-android",
            "desktop-browser",
        ]),
        "desktop-onboarding is an independent first-run feature crate",
    ),
];

/// Binaries and dev tools: nothing depends on them.
const BINARIES: &[&str] = &["patchwork", "remote", "oxidation"];

#[test]
fn crate_graph_follows_layers() {
    let graph = workspace_graph();
    // The parse found the graph, so an empty result below means the rules held.
    assert!(
        graph
            .get("backend")
            .is_some_and(|deps| deps.contains("platform")),
        "the test did not read backend's dependency on platform; the manifest parsing is broken"
    );

    let mut failures = Vec::new();
    for (krate, allow, why) in LAYERS {
        let Some(deps) = graph.get(*krate) else {
            failures.push(format!(
                "`{krate}` is in LAYERS but not in the workspace; update the rule"
            ));
            continue;
        };
        for dep in deps {
            let ok = match allow {
                Allow::Only(only) => only.contains(&dep.as_str()),
                Allow::Never(never) => !never.contains(&dep.as_str()),
            };
            if !ok {
                failures.push(format!("`{krate}` depends on `{dep}`: {why}"));
            }
        }
    }
    for (krate, deps) in &graph {
        for binary in BINARIES {
            if deps.contains(*binary) {
                failures.push(format!(
                    "`{krate}` depends on `{binary}`: it is a binary, so nothing may depend on it"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "crate layering broken:\n  {}",
        failures.join("\n  ")
    );
}

/// Files outside `crates/patchwork_backend/src/provider/` that still name `AgentKind::` variants.
///
/// Shrink this list; don't grow it. Ask `AgentKind::provider()`, `identity()` or
/// `capabilities()` instead of matching on the variant.
const AGENT_KIND_FILES: &[&str] = &[
    "crates/patchwork_backend/src/chats/delegate.rs",
    "crates/patchwork_backend/src/chats/granular.rs",
    "crates/patchwork_backend/src/chats/lifecycle.rs",
    "crates/patchwork_backend/src/chats/standing.rs",
    "crates/patchwork_backend/src/chats/title.rs",
    "crates/patchwork_backend/src/costs/prices.rs",
    "crates/patchwork_backend/src/extensions/hooks/folder.rs",
    "crates/patchwork_backend/src/gateway/inbound.rs",
    "crates/patchwork_backend/src/harness/agent/vision.rs",
    "crates/patchwork_backend/src/import/agy.rs",
    "crates/patchwork_backend/src/import/antigravity.rs",
    "crates/patchwork_backend/src/import/claude.rs",
    "crates/patchwork_backend/src/import/codex.rs",
    "crates/patchwork_backend/src/import/handoff.rs",
    "crates/patchwork_backend/src/import/t3.rs",
    "crates/patchwork_backend/src/import/types.rs",
    "crates/patchwork_daemon/src/logging/debug_log.rs",
    "crates/patchwork_backend/src/logging/report.rs",
    "crates/patchwork_backend/src/logging/system.rs",
    "crates/patchwork_backend/src/orchestration/approvals/answer.rs",
    "crates/patchwork_backend/src/orchestration/capabilities/integrations.rs",
    "crates/patchwork_backend/src/orchestration/changes/checkpoint.rs",
    "crates/patchwork_backend/src/orchestration/messages/send.rs",
    "crates/patchwork_backend/src/orchestration/output/ingest/replays.rs",
    "crates/patchwork_backend/src/orchestration/tasks/delegate.rs",
    "crates/patchwork_backend/src/orchestration/tasks/subagents.rs",
    "crates/patchwork_backend/src/orchestration/turn/end.rs",
    "crates/patchwork_backend/src/orchestration/turn/lifecycle.rs",
    "crates/patchwork_backend/src/orchestration/turn/process.rs",
    "crates/patchwork_backend/src/orchestration/turn/runner.rs",
    "crates/patchwork_backend/src/orchestration/turn/start/watch.rs",
    "crates/patchwork_backend/src/permissions/hook.rs",
    "crates/patchwork_backend/src/persistence/migrations/mod.rs",
    "crates/patchwork_backend/src/server/wire.rs",
    "crates/patchwork_backend/src/service.rs",
    "crates/patchwork_backend/src/settings/types.rs",
    "crates/patchwork_backend/src/usage/antigravity.rs",
    "crates/patchwork_backend/src/usage/cache.rs",
    "apps/desktop/src/chat/above_composer/in_terminal.rs",
    "apps/desktop/src/chat/composer/model_notice.rs",
    "apps/desktop/src/chat/composer/model_picker/layout.rs",
    "apps/desktop/src/chat/composer/model_picker/nav.rs",
    "apps/desktop/src/chat/composer/model_picker/rows.rs",
    "apps/desktop/src/chat/composer/sigil_menu/commands.rs",
    "apps/desktop/src/chat/composer/sigil_menu/layout.rs",
    "apps/desktop/src/chat/transcript/reply/subagent_cards.rs",
    "apps/desktop/src/sidebar/usage/limits.rs",
    "apps/desktop/src/sidebar/usage/popover.rs",
    "apps/desktop/crates/settings/src/settings/costs/rows.rs",
    "apps/desktop/crates/settings/src/settings/mcp/rows.rs",
    "apps/desktop/crates/settings/src/settings/mcp/servers.rs",
    "apps/desktop/crates/settings/src/settings/providers/auth.rs",
    "apps/desktop/crates/settings/src/settings/providers/catalog.rs",
    "apps/desktop/crates/settings/src/settings/providers/layout.rs",
    "apps/desktop/crates/settings/src/settings/providers/rows.rs",
    "apps/desktop/crates/settings/src/settings/tokens.rs",
    "apps/desktop/crates/onboarding/src/steps.rs",
];

/// Files named for a layer rather than what they hold.
///
/// Shrink this list; don't grow it. Name a new file for its contents, such as `commit_box.rs`.
const LAYER_FILES: &[&str] = &[
    "crates/patchwork_backend/src/agents/types.rs",
    "crates/patchwork_backend/src/browser/types.rs",
    "crates/patchwork_backend/src/chats/types.rs",
    "crates/patchwork_backend/src/costs/types.rs",
    "crates/patchwork_backend/src/gateway/types.rs",
    "crates/patchwork_backend/src/harness/agent/request/types.rs",
    "crates/patchwork_backend/src/import/types.rs",
    "crates/patchwork_backend/src/orchestration/capabilities/types.rs",
    "crates/patchwork_backend/src/permissions/types.rs",
    "crates/patchwork_backend/src/projects/types.rs",
    "crates/patchwork_backend/src/quilts/render.rs",
    "crates/patchwork_backend/src/quilts/types.rs",
    "crates/patchwork_backend/src/scheduled/types.rs",
    "crates/patchwork_backend/src/settings/types.rs",
    "crates/patchwork_backend/src/terminal/types.rs",
    "crates/patchwork_backend/src/usage/types.rs",
];

/// File names that say which layer a file is in, not what it holds.
const LAYER_NAMES: &[&str] = &[
    "types.rs",
    "utils.rs",
    "helpers.rs",
    "render.rs",
    "misc.rs",
    "common.rs",
];

/// Calls that write the settings without `Core::update_settings`, and the files that may make them.
///
/// `save_settings` is the raw table write; `Store::update_settings_as` saves without applying what
/// changed, which only the core's own save path and the vault's sealing may do.
const SETTINGS_WRITES: &[(&str, &[&str])] = &[
    (
        "save_settings(",
        &[
            "crates/patchwork_backend/src/persistence/settings.rs",
            "crates/patchwork_backend/src/persistence/store.rs",
        ],
    ),
    (
        "update_settings_as(",
        &[
            "crates/patchwork_backend/src/persistence/store.rs",
            "crates/patchwork_backend/src/settings/save.rs",
            "crates/patchwork_backend/src/settings/seal.rs",
        ],
    ),
];

/// Every `.rs` file under `dir`, with its path from the workspace root.
fn rust_files(root: &Path, dir: &Path, into: &mut Vec<(String, PathBuf)>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(root, &path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            into.push((relative, path));
        }
    }
}

/// Every `.rs` file in every workspace member's `src/`, sorted by path.
fn source_files() -> Vec<(String, String)> {
    let root = fs::canonicalize(root()).expect("workspace root");
    let workspace = read_toml(&root.join("Cargo.toml"));
    let members: Vec<&str> = workspace["workspace"]["members"]
        .as_array()
        .expect("workspace.members is a list")
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    let mut files = Vec::new();
    for member in members {
        let src = root.join(member).join("src");
        if src.is_dir() {
            rust_files(&root, &src, &mut files);
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|(relative, path)| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            (relative, text)
        })
        .collect()
}

/// Checks that `found` matches `allowed` exactly: new entries fail, and so do stale ones.
fn check_allowlist(
    rule: &str,
    list: &str,
    found: &BTreeSet<&str>,
    allowed: &[&str],
    failures: &mut Vec<String>,
) {
    for path in found {
        if !allowed.contains(path) {
            failures.push(format!("{path}: {rule}"));
        }
    }
    for path in allowed {
        if !found.contains(path) {
            failures.push(format!(
                "{path} no longer breaks \"{rule}\"; remove it from {list}"
            ));
        }
    }
}

#[test]
fn source_follows_rules() {
    let files = source_files();
    assert!(
        files
            .iter()
            .any(|(path, _)| path == "crates/patchwork_backend/src/lib.rs"),
        "the test did not find crates/patchwork_backend/src/lib.rs; the source walk is broken"
    );
    let mut failures = Vec::new();

    let agent_kind: BTreeSet<&str> = files
        .iter()
        .filter(|(path, text)| {
            !path.starts_with("crates/patchwork_backend/src/provider/")
                && !path.starts_with("crates/provider_")
                && text.contains("AgentKind::")
        })
        .map(|(path, _)| path.as_str())
        .collect();
    check_allowlist(
        "names an `AgentKind::` variant outside crates/patchwork_backend/src/provider; ask the provider instead",
        "AGENT_KIND_FILES",
        &agent_kind,
        AGENT_KIND_FILES,
        &mut failures,
    );

    let layer: BTreeSet<&str> = files
        .iter()
        .filter(|(path, _)| {
            LAYER_NAMES
                .iter()
                .any(|name| path.rsplit('/').next() == Some(*name))
        })
        .map(|(path, _)| path.as_str())
        .collect();
    check_allowlist(
        "is named for a layer; name the file for what it holds",
        "LAYER_FILES",
        &layer,
        LAYER_FILES,
        &mut failures,
    );

    for (path, text) in &files {
        for (number, line) in text.lines().enumerate() {
            let line_no = number + 1;
            if line.contains("#[allow(") || line.contains("#![allow(") {
                failures.push(format!(
                    "{path}:{line_no}: uses `#[allow]`; use `#[expect(..., reason = \"...\")]`"
                ));
            }
            for (call, allowed) in SETTINGS_WRITES {
                if line.contains(call) && !allowed.contains(&path.as_str()) {
                    failures.push(format!(
                        "{path}:{line_no}: calls `{call}..)`, which skips `Core::update_settings`; \
                         change the settings through it"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "source rules broken:\n  {}",
        failures.join("\n  ")
    );
}
