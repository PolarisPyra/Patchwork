//! What to run to check a project: the first of the well-known check commands the folder has.
//!
//! The order puts the command that says the most first: a project's own `check` script (typecheck,
//! lint and tests together), then its tests, then the language's own runner. A folder with none of
//! them has nothing to run, and its replies say so by saying nothing rather than by claiming a
//! pass.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::Path;

/// A check command, and how it was arrived at, for a person to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The command line, as the person would type it in the project's folder.
    pub line: String,
}

impl Command {
    fn new(line: impl Into<String>) -> Self {
        Self { line: line.into() }
    }
}

/// The check command for the project at `root`, or None when there is nothing to run.
#[must_use]
pub fn command(root: &Path) -> Option<Command> {
    detect(&Local(root))
}

/// [`command`] for a project root as Patchwork keeps it, on this machine or an SSH host.
#[must_use]
pub fn command_at(root: &str) -> Option<Command> {
    match crate::system::remote::split(root) {
        Some(at) => detect(&Remote::read(at.host, at.path)?),
        None => command(Path::new(root)),
    }
}

fn detect(root: &dyn Probe) -> Option<Command> {
    node_scripts(root)
        .or_else(|| {
            root.is_file("Cargo.toml")
                .then(|| Command::new("cargo test"))
        })
        .or_else(|| python(root, PYTHON_MANIFESTS.iter().any(|name| root.is_file(name))))
        .or_else(|| {
            root.is_file("go.mod")
                .then(|| Command::new("go test ./..."))
        })
        .or_else(|| make(root))
        .or_else(|| root.is_file("mix.exs").then(|| Command::new("mix test")))
        // A folder of tests is a Python project's signal only when nothing else matched: Go, C
        // and Elixir projects have one too.
        .or_else(|| python(root, root.is_dir("tests")))
}

/// The files that make a folder a Python project.
const PYTHON_MANIFESTS: [&str; 5] = [
    "pyproject.toml",
    "setup.cfg",
    "pytest.ini",
    "tox.ini",
    "setup.py",
];

/// What [`detect`] looks for, in its order, for a person to read.
pub(super) fn looked_for() -> String {
    let (last, rest) = PYTHON_MANIFESTS
        .split_last()
        .map_or(("", &[][..]), |(l, r)| (*l, r));
    format!(
        "package.json scripts (check, then test), Cargo.toml, a Python manifest ({} or {last}), \
         go.mod, a Makefile's check or test target, mix.exs, and a tests folder.",
        rest.join(", ")
    )
}

/// A `check` script in `package.json`, else a `test` script, with the package manager the project
/// locks its dependencies with.
fn node_scripts(root: &dyn Probe) -> Option<Command> {
    let text = root.read("package.json")?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    let scripts = manifest.get("scripts").filter(|s| s.is_object())?;
    let runner = node_runner(root);
    let script = ["check", "test"].into_iter().find(|name| {
        scripts
            .get(*name)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    })?;
    // npm runs a script by name; the others run it with `run` when the name is not their own.
    let line = match (runner, script) {
        ("npm", "test") => "npm test".to_string(),
        ("bun", "test") => "bun run test".to_string(),
        (runner, script) => format!("{runner} run {script}"),
    };
    Some(Command::new(line))
}

/// The package manager a Node project uses, by its lockfile; npm unless one is found and
/// installed, since a lockfile can outlive the tool it names.
fn node_runner(root: &dyn Probe) -> &'static str {
    const RUNNERS: [(&str, &str); 3] = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        // Bun writes either, depending on its version.
        ("bun.lockb", "bun"),
    ];
    RUNNERS
        .iter()
        .find(|(lockfile, runner)| root.is_file(lockfile) && root.installed(runner))
        .map(|(_, runner)| *runner)
        .or_else(|| (root.is_file("bun.lock") && root.installed("bun")).then_some("bun"))
        .unwrap_or("npm")
}

/// A Python project's tests, run through the interpreter the project is set up for, when `found`
/// says the folder is one.
fn python(root: &dyn Probe, found: bool) -> Option<Command> {
    if !found {
        return None;
    }
    // macOS and some Linux installs have only `python3`.
    let interpreter = if root.installed("python") {
        "python"
    } else {
        "python3"
    };
    Some(Command::new(format!("{interpreter} -m pytest -q")))
}

/// A Makefile's `check` target, else its `test` target.
fn make(root: &dyn Probe) -> Option<Command> {
    let text = root.read("Makefile")?;
    let has = |target: &str| {
        text.lines().any(|line| {
            let Some((name, rest)) = line.split_once(':') else {
                return false;
            };
            // A rule, not a variable (`VAR := x`) or a continuation of another rule.
            name.trim() == target && !rest.starts_with('=')
        })
    };
    ["check", "test"]
        .into_iter()
        .find(|target| has(target))
        .map(|target| Command::new(format!("make {target}")))
}

/// Whether `program` is on the PATH, so a lockfile naming a package manager that is not installed
/// does not decide what runs.
fn installed(program: &str) -> bool {
    crate::system::find_program(program).is_some()
}

/// What detection asks of a project's folder.
trait Probe {
    /// The text of the file `name` in the folder.
    fn read(&self, name: &str) -> Option<String>;
    fn is_file(&self, name: &str) -> bool;
    fn is_dir(&self, name: &str) -> bool;
    /// Whether `program` is on the PATH the project's commands run with.
    fn installed(&self, program: &str) -> bool;
}

/// A folder on this machine.
struct Local<'a>(&'a Path);

impl Probe for Local<'_> {
    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(name)).ok()
    }

    fn is_file(&self, name: &str) -> bool {
        self.0.join(name).is_file()
    }

    fn is_dir(&self, name: &str) -> bool {
        self.0.join(name).is_dir()
    }

    fn installed(&self, program: &str) -> bool {
        installed(program)
    }
}

/// The files detection looks for, besides [`PYTHON_MANIFESTS`].
const LOOKED_FOR: [&str; 9] = [
    "package.json",
    "Cargo.toml",
    "go.mod",
    "Makefile",
    "mix.exs",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "bun.lock",
];

/// The programs detection asks about.
const PROGRAMS: [&str; 4] = ["pnpm", "yarn", "bun", "python"];

/// A folder on an SSH host, read in one command: which of [`LOOKED_FOR`] and
/// [`PYTHON_MANIFESTS`] it has, whether it has
/// a `tests` folder, which of [`PROGRAMS`] the user's login shell finds, and the two files
/// detection reads.
struct Remote {
    files: HashSet<String>,
    tests: bool,
    programs: HashSet<String>,
    package: Option<String>,
    makefile: Option<String>,
}

impl Remote {
    fn read(host: &str, dir: &str) -> Option<Self> {
        let mut names = LOOKED_FOR.to_vec();
        names.extend(PYTHON_MANIFESTS);
        names.push("tests");
        let found = crate::system::remote::op::survey(
            host,
            dir,
            &names,
            &PROGRAMS,
            &["package.json", "Makefile"],
            262_144,
        )
        .ok()?;
        let mut texts = found.texts.into_iter();
        Some(Self {
            tests: found.dirs.iter().any(|d| d == "tests"),
            files: found.files.into_iter().collect(),
            programs: found.programs.into_iter().collect(),
            package: texts.next().flatten(),
            makefile: texts.next().flatten(),
        })
    }
}

impl Probe for Remote {
    fn read(&self, name: &str) -> Option<String> {
        match name {
            "package.json" => self.package.clone(),
            "Makefile" => self.makefile.clone(),
            _ => None,
        }
    }

    fn is_file(&self, name: &str) -> bool {
        self.files.contains(name)
    }

    fn is_dir(&self, name: &str) -> bool {
        name == "tests" && self.tests
    }

    fn installed(&self, program: &str) -> bool {
        self.programs.contains(program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(crate::system::new_id());
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn a_check_script_comes_before_the_tests() {
        let dir = project(&[(
            "package.json",
            r#"{"scripts":{"test":"node --test","check":"tsc && node --test"}}"#,
        )]);
        assert_eq!(command(&dir), Some(Command::new("npm run check")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_test_script_runs_as_the_package_manager_runs_it() {
        let dir = project(&[("package.json", r#"{"scripts":{"test":"node --test"}}"#)]);
        assert_eq!(command(&dir), Some(Command::new("npm test")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_package_json_without_scripts_falls_through_to_the_language() {
        let dir = project(&[
            ("package.json", r#"{"name":"site"}"#),
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
        ]);
        assert_eq!(command(&dir), Some(Command::new("cargo test")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cargo_project_runs_its_tests() {
        let dir = project(&[("Cargo.toml", "[package]\nname = \"x\"\n")]);
        assert_eq!(command(&dir), Some(Command::new("cargo test")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_python_project_runs_pytest_through_the_interpreter() {
        let dir = project(&[("pyproject.toml", "[project]\nname = \"x\"\n")]);
        let interpreter = if installed("python") {
            "python"
        } else {
            "python3"
        };
        assert_eq!(
            command(&dir),
            Some(Command::new(format!("{interpreter} -m pytest -q")))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_makefile_target_is_a_rule_and_not_a_variable() {
        let rules = project(&[("Makefile", "check:\n\tcargo xtask ci\n")]);
        assert_eq!(command(&rules), Some(Command::new("make check")));
        let variables = project(&[("Makefile", "check := 1\nall:\n\techo hi\n")]);
        assert_eq!(command(&variables), None);
        let _ = std::fs::remove_dir_all(rules);
        let _ = std::fs::remove_dir_all(variables);
    }

    #[test]
    fn an_empty_folder_has_nothing_to_run() {
        let dir = project(&[]);
        assert_eq!(command(&dir), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_lockfile_for_a_tool_that_is_not_installed_does_not_decide() {
        // A pnpm lockfile with no pnpm on the PATH still runs npm: the project can be cloned
        // without the tool the lockfile names.
        let dir = project(&[
            ("package.json", r#"{"scripts":{"test":"node --test"}}"#),
            ("pnpm-lock.yaml", "lockfileVersion: '9.0'\n"),
        ]);
        let expected = if installed("pnpm") {
            "pnpm run test"
        } else {
            "npm test"
        };
        assert_eq!(command(&dir), Some(Command::new(expected)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
