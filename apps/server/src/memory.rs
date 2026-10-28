//! The memory tree: every place an agent takes standing instructions from, as a tree the app can
//! browse. These are the files the agents already read — a project's `AGENTS.md` or
//! `CLAUDE.md`, the user's own global instruction files, the skills folder, and plugins' skills —
//! so the tree shows what is really loaded rather than a second copy of it.

use std::path::{Path, PathBuf};

use crate::types::{MemoryKind, MemoryNode, Settings};

/// Instruction files an agent reads from a folder, in the order it reads them.
const INSTRUCTION_FILES: [&str; 3] = ["AGENTS.md", "CLAUDE.md", "CONTEXT.md"];

/// Where the agents' own global instructions live, relative to the home folder.
const GLOBAL_FILES: [&str; 5] = [
    ".codex/AGENTS.md",
    ".claude/CLAUDE.md",
    ".config/opencode/AGENTS.md",
    ".config/patchwork/AGENTS.md",
    "AGENTS.md",
];

/// How deep a project is searched for nested instruction files.
const MAX_DEPTH: usize = 3;

/// One file in the tree, with its size and a note on what it is for.
fn file_node(name: &str, path: &Path, note: Option<String>) -> MemoryNode {
    let size = std::fs::metadata(path).ok().map(|meta| meta.len());
    MemoryNode {
        name: name.to_string(),
        path: Some(path.to_string_lossy().into_owned()),
        kind: MemoryKind::File,
        note,
        size,
        children: Vec::new(),
    }
}

/// A heading: a node with children and no file of its own.
fn group(name: &str, children: Vec<MemoryNode>) -> MemoryNode {
    MemoryNode {
        name: name.to_string(),
        path: None,
        kind: MemoryKind::Folder,
        note: None,
        size: None,
        children,
    }
}

/// The instruction files in `root`, skipping the folders that never hold hand-written notes.
fn instructions_in(root: &Path) -> Vec<MemoryNode> {
    INSTRUCTION_FILES
        .iter()
        .filter_map(|name| {
            let path = root.join(name);
            path.is_file().then(|| file_node(name, &path, None))
        })
        .collect()
}

/// Nested instruction files under `root`, as `path/file` names, up to `MAX_DEPTH` folders deep.
fn nested_instructions(root: &Path) -> Vec<MemoryNode> {
    let mut found = Vec::new();
    let mut queue: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = queue.pop() {
        if depth >= MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            for file in INSTRUCTION_FILES {
                let nested = path.join(file);
                if nested.is_file() {
                    let shown = nested
                        .strip_prefix(root)
                        .unwrap_or(&nested)
                        .to_string_lossy()
                        .into_owned();
                    found.push(file_node(&shown, &nested, None));
                }
            }
            queue.push((path, depth + 1));
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The global instruction files the agents read, under a `Global` heading.
fn global(settings: &Settings) -> Vec<MemoryNode> {
    let mut nodes = Vec::new();
    if let Some(home) = crate::provider::home_dir() {
        for relative in GLOBAL_FILES {
            let path = home.join(relative);
            if path.is_file() {
                nodes.push(file_node(relative, &path, None));
            }
        }
    }
    if let Some(config) = crate::skills::config_dir() {
        // Patchwork's own notes for the API agent, which has no CLI config of its own.
        let path = config.join("AGENTS.md");
        if path.is_file() {
            nodes.push(file_node(
                "patchwork/AGENTS.md",
                &path,
                Some("Patchwork's API agent".into()),
            ));
        }
    }
    if settings.computer_use
        && let Some(config) = crate::skills::config_dir()
    {
        let path = config.join("computer-use.md");
        if path.is_file() {
            nodes.push(file_node(
                "patchwork/computer-use.md",
                &path,
                Some("What the agent knows about the screen".into()),
            ));
        }
    }
    nodes
}

/// A project's instruction files: its own, and the nested ones that override them.
fn project(name: &str, root: &Path) -> MemoryNode {
    let mut children = instructions_in(root);
    children.extend(nested_instructions(root));
    let mut node = group(name, children);
    node.path = Some(root.to_string_lossy().into_owned());
    node.note = Some(root.to_string_lossy().into_owned());
    node
}

/// Skills as they load, each with the path to its `SKILL.md`.
fn skill_nodes(settings: &Settings) -> Vec<MemoryNode> {
    crate::skills::all(settings)
        .into_iter()
        .map(|skill| MemoryNode {
            name: skill.name,
            path: Some(skill.path),
            kind: MemoryKind::Skill,
            note: skill.description,
            size: None,
            children: Vec::new(),
        })
        .collect()
}

/// The whole tree: global notes, each project, the skills, and the plugins.
pub fn tree(settings: &Settings) -> Vec<MemoryNode> {
    let mut roots = Vec::new();

    let globals = global(settings);
    if !globals.is_empty() {
        roots.push(group("Global", globals));
    }

    let mut projects = Vec::new();
    for known in &settings.projects {
        let root = Path::new(&known.path);
        if !root.is_dir() {
            continue;
        }
        let node = project(&known.name, root);
        if !node.children.is_empty() {
            projects.push(node);
        }
    }
    if !projects.is_empty() {
        roots.push(group("Projects", projects));
    }

    let skills = skill_nodes(settings);
    if !skills.is_empty() {
        roots.push(group("Skills", skills));
    }

    let plugins: Vec<MemoryNode> = crate::plugins::list(settings)
        .into_iter()
        .filter(|plugin| plugin.enabled && !plugin.skills.is_empty())
        .map(|plugin| {
            let children = plugin
                .skills
                .into_iter()
                .map(|skill| MemoryNode {
                    name: skill.name,
                    path: Some(skill.path),
                    kind: MemoryKind::Skill,
                    note: skill.description,
                    size: None,
                    children: Vec::new(),
                })
                .collect();
            group(&plugin.name, children)
        })
        .collect();
    if !plugins.is_empty() {
        roots.push(group("Plugins", plugins));
    }

    roots
}

/// Reads a file the tree lists. Refuses any other path, so the viewer cannot be pointed at the
/// rest of the disk.
pub fn read(settings: &Settings, path: &str) -> Result<String, String> {
    if !listed(settings, Path::new(path)) {
        return Err("That file is not part of the memory tree".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {path}: {e}"))?;
    // A browser only shows the start of a very long file; the tree is for reading notes.
    Ok(text)
}

/// Whether `path` is one of the tree's files.
fn listed(settings: &Settings, path: &Path) -> bool {
    fn walk(nodes: &[MemoryNode], path: &Path) -> bool {
        nodes.iter().any(|node| {
            node.path
                .as_deref()
                .is_some_and(|listed| Path::new(listed) == path && node.kind != MemoryKind::Folder)
                || walk(&node.children, path)
        })
    }
    walk(&tree(settings), path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Project;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(crate::types::new_id());
        std::fs::create_dir_all(&dir).expect("a temp dir");
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(path, text).expect("a file");
    }

    fn settings_for(project: &Path) -> Settings {
        Settings {
            projects: vec![Project {
                id: "p".into(),
                name: "Demo".into(),
                path: project.to_string_lossy().into_owned(),
            }],
            skills_dir: Some(project.join("nowhere").to_string_lossy().into_owned()),
            plugins_dir: Some(project.join("no-plugins").to_string_lossy().into_owned()),
            ..Settings::default()
        }
    }

    /// The names under a heading, joined, for a short assertion.
    fn names(nodes: &[MemoryNode], heading: &str) -> String {
        nodes
            .iter()
            .find(|node| node.name == heading)
            .map(|node| {
                node.children
                    .iter()
                    .map(|child| child.name.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_project_lists_its_instruction_files() {
        let root = temp_dir();
        write(&root.join("AGENTS.md"), "# Rules\n");
        let roots = tree(&settings_for(&root));
        assert_eq!(names(&roots, "Projects"), "Demo");
        let projects = roots
            .iter()
            .find(|node| node.name == "Projects")
            .expect("projects");
        let files: Vec<&str> = projects.children[0]
            .children
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        assert_eq!(files, vec!["AGENTS.md"]);
    }

    #[test]
    fn nested_files_are_shown_with_their_folder() {
        let root = temp_dir();
        write(&root.join("AGENTS.md"), "# Rules\n");
        write(&root.join("crates/app/CLAUDE.md"), "# App rules\n");
        // Folders that hold no notes are not searched into.
        write(&root.join("node_modules/x/AGENTS.md"), "# Ignored\n");
        let roots = tree(&settings_for(&root));
        let projects = roots
            .iter()
            .find(|node| node.name == "Projects")
            .expect("projects");
        let nested: Vec<&str> = projects.children[0]
            .children
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        assert!(nested.contains(&"crates/app/CLAUDE.md"), "{nested:?}");
        assert!(!nested.iter().any(|name| name.contains("node_modules")));
    }

    #[test]
    fn only_listed_files_can_be_read() {
        let root = temp_dir();
        write(&root.join("AGENTS.md"), "# Rules\n");
        write(&root.join("secret.txt"), "nope\n");
        let settings = settings_for(&root);
        let listed = root.join("AGENTS.md");
        assert_eq!(
            read(&settings, &listed.to_string_lossy()).unwrap(),
            "# Rules\n"
        );
        let other = root.join("secret.txt");
        assert!(read(&settings, &other.to_string_lossy()).is_err());
    }

    #[test]
    fn a_missing_project_folder_is_left_out() {
        let missing = PathBuf::from("/nonexistent-patchwork-project");
        let roots = tree(&settings_for(&missing));
        assert!(!roots.iter().any(|node| node.name == "Projects"));
    }
}
