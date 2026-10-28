//! Plugins: a folder with a `plugin.json`, its skills, and the MCP servers it adds. The format
//! is Codex's `.codex-plugin/plugin.json` where it can be, so a Codex bundle works as it is.
//!
//! Bundled plugins ship inside the app and are written into the plugins folder on launch, the
//! way Claude desktop writes its bundled skills. A bundled plugin that the user has deleted is
//! not written again, so removing one sticks.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::skills::{self, Skill};
use crate::types::{Plugin, PluginMcpServer, Settings};

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remembers the config folder and writes the bundled plugins into it.
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    seed();
}

fn default_dir() -> PathBuf {
    CONFIG_DIR
        .get()
        .cloned()
        .unwrap_or_default()
        .join("plugins")
}

/// The plugins folder in use.
pub fn dir(settings: &Settings) -> PathBuf {
    match settings.plugins_dir.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => default_dir(),
    }
}

/// Where `plugin.json` sits inside a bundle: Patchwork's own place, then Codex's.
const MANIFESTS: [&str; 2] = [".patchwork-plugin/plugin.json", ".codex-plugin/plugin.json"];

/// Codex writes `plugin.json` as a flat file in some bundles; so do we.
const ROOT_MANIFEST: &str = "plugin.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    /// The skills a plugin ships, as Codex lists them. The folder's `skills/` is read instead
    /// when this is missing.
    #[serde(default)]
    skills: Vec<SkillRef>,
    /// MCP servers the plugin adds, in Codex's shape.
    #[serde(default)]
    mcp_servers: Vec<McpServerRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillRef {
    name: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerRef {
    name: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    url: Option<String>,
}

/// Reads one bundle's folder. `None` when it has no manifest.
fn read_bundle(dir: &Path, enabled: bool, bundled: bool) -> Option<Plugin> {
    let manifest_path = MANIFESTS
        .iter()
        .map(|rel| dir.join(rel))
        .chain(std::iter::once(dir.join(ROOT_MANIFEST)))
        .find(|path| path.is_file())?;
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest: Manifest = serde_json::from_str(&text).ok()?;
    let id = dir.file_name()?.to_string_lossy().into_owned();
    let mut skills = skills::list(&dir.join("skills"));
    // A manifest may point at skills elsewhere in the bundle or in the skills folder.
    for reference in &manifest.skills {
        let Some(path) = reference.path.as_deref() else {
            continue;
        };
        let skill_md = dir.join(path).join("SKILL.md");
        if !skill_md.is_file()
            || skills
                .iter()
                .any(|skill| skill.path == skill_md.to_string_lossy())
        {
            continue;
        }
        let (name, description) = skills::frontmatter(&std::fs::read_to_string(&skill_md).ok()?);
        skills.push(Skill {
            name: reference
                .name
                .clone()
                .or(name)
                .unwrap_or_else(|| id.clone()),
            description,
            path: skill_md.to_string_lossy().into_owned(),
            enabled: true,
        });
    }
    for skill in &mut skills {
        skill.enabled = enabled;
    }
    let mcp_servers = manifest
        .mcp_servers
        .iter()
        .filter_map(|server| {
            Some(PluginMcpServer {
                name: server.name.clone().or_else(|| server.command.clone())?,
                command: server.command.clone().or_else(|| server.url.clone())?,
                args: server.args.clone(),
            })
        })
        .collect();
    Some(Plugin {
        id,
        name: manifest.name.unwrap_or_else(|| "Plugin".into()),
        version: manifest.version,
        description: manifest.description.filter(|text| !text.trim().is_empty()),
        path: dir.to_string_lossy().into_owned(),
        skills,
        mcp_servers,
        enabled,
        bundled,
    })
}

/// Every plugin in the plugins folder, by name, with its skills as `SKILL.md` folders.
pub fn list(settings: &Settings) -> Vec<Plugin> {
    let root = dir(settings);
    let mut plugins: Vec<Plugin> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let path = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            let enabled = !settings.disabled_plugins.contains(&id);
            let bundled = BUNDLED.iter().any(|bundle| bundle.id == id);
            read_bundle(&path, enabled, bundled)
        })
        .collect();
    plugins.sort_by_key(|plugin| plugin.name.to_lowercase());
    plugins
}

/// The skills of the enabled plugins, which load beside the skills folder's own.
pub fn skills(settings: &Settings) -> Vec<Skill> {
    let mut skills: Vec<Skill> = list(settings)
        .into_iter()
        .filter(|plugin| plugin.enabled)
        .flat_map(|plugin| plugin.skills)
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// The bundle an app ships with: an id, its `plugin.json`, and its skills as (folder, `SKILL.md`).
struct Bundled {
    id: &'static str,
    manifest: &'static str,
    skills: &'static [(&'static str, &'static str)],
}

const BUNDLED: &[Bundled] = &[Bundled {
    id: "patchwork-review",
    manifest: include_str!("../plugins/patchwork-review/plugin.json"),
    skills: &[
        (
            "review-diff",
            include_str!("../plugins/patchwork-review/skills/review-diff/SKILL.md"),
        ),
        (
            "scheduled-report",
            include_str!("../plugins/patchwork-review/skills/scheduled-report/SKILL.md"),
        ),
    ],
}];

/// Lists the bundled plugins this folder has already been given, so one the user deleted is not
/// written back. It is a file rather than the folders themselves: a folder that is gone cannot say
/// it was ever there.
const SEEDED: &str = ".seeded";

fn seeded(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join(SEEDED))
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Writes the bundled plugins this folder has not been given before. A folder the user deleted
/// stays deleted, and one they edited is left alone.
fn seed() {
    let root = default_dir();
    let mut given = seeded(&root);
    for bundle in BUNDLED {
        if given.iter().any(|id| id == bundle.id) {
            continue;
        }
        let folder = root.join(bundle.id);
        let manifest = folder.join(".patchwork-plugin");
        if std::fs::create_dir_all(&manifest).is_err() {
            continue;
        }
        if std::fs::write(manifest.join("plugin.json"), bundle.manifest).is_err() {
            let _ = std::fs::remove_dir_all(&folder);
            continue;
        }
        for (name, text) in bundle.skills {
            let skill = folder.join("skills").join(name);
            if std::fs::create_dir_all(&skill).is_err()
                || std::fs::write(skill.join("SKILL.md"), text).is_err()
            {
                let _ = std::fs::remove_dir_all(&folder);
                return;
            }
        }
        given.push(bundle.id.to_string());
    }
    if given.is_empty() {
        return;
    }
    if std::fs::create_dir_all(&root).is_ok() {
        let text: String = given.iter().map(|id| format!("{id}\n")).collect();
        let _ = std::fs::write(root.join(SEEDED), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(crate::types::new_id());
        std::fs::create_dir_all(&dir).expect("a temp dir");
        dir
    }

    fn settings_with(plugins_dir: PathBuf, disabled: &[&str]) -> Settings {
        Settings {
            plugins_dir: Some(plugins_dir.to_string_lossy().into_owned()),
            disabled_plugins: disabled.iter().map(|id| (*id).to_string()).collect(),
            ..Settings::default()
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(path, text).expect("a file");
    }

    #[test]
    fn reads_a_bundle_and_its_skills() {
        let root = temp_dir();
        write(
            &root.join("demo/.patchwork-plugin/plugin.json"),
            r#"{"name":"Demo","version":"1.2.3","description":"Two skills"}"#,
        );
        write(
            &root.join("demo/skills/one/SKILL.md"),
            "---\nname: one\ndescription: The first\n---\n\nBody.\n",
        );
        let plugins = list(&settings_with(root, &[]));
        assert_eq!(plugins.len(), 1);
        let plugin = &plugins[0];
        assert_eq!(plugin.id, "demo");
        assert_eq!(plugin.name, "Demo");
        assert_eq!(plugin.version.as_deref(), Some("1.2.3"));
        assert_eq!(plugin.skills.len(), 1);
        assert_eq!(plugin.skills[0].name, "one");
        assert_eq!(plugin.skills[0].description.as_deref(), Some("The first"));
        assert!(plugin.enabled);
        assert!(!plugin.bundled);
    }

    #[test]
    fn reads_a_codex_bundle_with_servers() {
        let root = temp_dir();
        write(
            &root.join("codexish/.codex-plugin/plugin.json"),
            r#"{"name":"Codexish","mcpServers":[{"name":"tools","command":"./run"}]}"#,
        );
        let plugins = list(&settings_with(root, &[]));
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "codexish");
        assert_eq!(plugins[0].mcp_servers[0].name, "tools");
        assert_eq!(plugins[0].mcp_servers[0].command, "./run");
    }

    #[test]
    fn a_disabled_plugin_contributes_no_skills() {
        let root = temp_dir();
        write(
            &root.join("demo/.patchwork-plugin/plugin.json"),
            r#"{"name":"Demo"}"#,
        );
        write(
            &root.join("demo/skills/one/SKILL.md"),
            "---\nname: one\n---\n\nBody.\n",
        );
        let settings = settings_with(root, &["demo"]);
        assert!(!list(&settings)[0].enabled);
        assert!(skills(&settings).is_empty());
    }

    /// Seeds the bundled plugin once per process: `init` remembers its folder in a `OnceLock`.
    #[test]
    fn the_bundled_plugin_is_written_once_and_left_alone_after() {
        let config = temp_dir();
        init(&config);
        let folder = config.join("plugins/patchwork-review");
        let manifest = folder.join(".patchwork-plugin/plugin.json");
        assert!(
            manifest.is_file(),
            "the bundled plugin's manifest is written"
        );
        assert!(
            folder.join("skills/review-diff/SKILL.md").is_file(),
            "its skills are written"
        );
        // A second launch leaves what is there, including a deletion the user made.
        std::fs::remove_dir_all(&folder).expect("remove the seeded plugin");
        init(&config);
        assert!(!folder.exists(), "a plugin the user deleted stays deleted");
    }

    #[test]
    fn a_folder_without_a_manifest_is_not_a_plugin() {
        let root = temp_dir();
        write(
            &root.join("plain/skills/one/SKILL.md"),
            "---\nname: one\n---\n",
        );
        assert!(list(&settings_with(root, &[])).is_empty());
    }
}
