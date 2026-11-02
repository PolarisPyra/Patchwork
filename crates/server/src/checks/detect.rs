//! What to run to check a project: the first of the well-known check commands the folder has.
//!
//! The order puts the command that says the most first: a project's own `check` script (typecheck,
//! lint and tests together), then its tests, then the language's own runner. A folder with none of
//! them has nothing to run, and its replies say so by saying nothing rather than by claiming a
//! pass.

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
pub fn command(root: &Path) -> Option<Command> {
    node_scripts(root)
        .or_else(|| file(root, "Cargo.toml").then(|| Command::new("cargo test")))
        .or_else(|| python(root))
        .or_else(|| file(root, "go.mod").then(|| Command::new("go test ./...")))
        .or_else(|| make(root))
        .or_else(|| file(root, "mix.exs").then(|| Command::new("mix test")))
}

/// A `check` script in `package.json`, else a `test` script, with the package manager the project
/// locks its dependencies with.
fn node_scripts(root: &Path) -> Option<Command> {
    let text = std::fs::read_to_string(root.join("package.json")).ok()?;
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
fn node_runner(root: &Path) -> &'static str {
    const RUNNERS: [(&str, &str); 3] = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        // Bun writes either, depending on its version.
        ("bun.lockb", "bun"),
    ];
    RUNNERS
        .iter()
        .find(|(lockfile, runner)| file(root, lockfile) && installed(runner))
        .map(|(_, runner)| *runner)
        .or_else(|| (file(root, "bun.lock") && installed("bun")).then_some("bun"))
        .unwrap_or("npm")
}

/// A Python project's tests, run through the interpreter the project is set up for.
fn python(root: &Path) -> Option<Command> {
    let configured = [
        "pyproject.toml",
        "setup.cfg",
        "pytest.ini",
        "tox.ini",
        "setup.py",
    ]
    .into_iter()
    .any(|name| file(root, name));
    // A folder of tests is a Python project's own signal, for the ones with no manifest.
    let folder = root.join("tests").is_dir();
    if !configured && !folder {
        return None;
    }
    // macOS and some Linux installs have only `python3`.
    let interpreter = if installed("python") {
        "python"
    } else {
        "python3"
    };
    Some(Command::new(format!("{interpreter} -m pytest -q")))
}

/// A Makefile's `check` target, else its `test` target.
fn make(root: &Path) -> Option<Command> {
    let text = std::fs::read_to_string(root.join("Makefile")).ok()?;
    let has = |target: &str| {
        text.lines().any(|line| {
            let (name, rest) = match line.split_once(':') {
                Some(split) => split,
                None => return false,
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

fn file(root: &Path, name: &str) -> bool {
    root.join(name).is_file()
}

/// Whether `program` is on the PATH, so a lockfile naming a package manager that is not installed
/// does not decide what runs.
fn installed(program: &str) -> bool {
    which::which_in(
        program,
        crate::system::path_var(),
        std::env::current_dir().unwrap_or_default(),
    )
    .is_ok()
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
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_test_script_runs_as_the_package_manager_runs_it() {
        let dir = project(&[("package.json", r#"{"scripts":{"test":"node --test"}}"#)]);
        assert_eq!(command(&dir), Some(Command::new("npm test")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_package_json_without_scripts_falls_through_to_the_language() {
        let dir = project(&[
            ("package.json", r#"{"name":"site"}"#),
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
        ]);
        assert_eq!(command(&dir), Some(Command::new("cargo test")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_cargo_project_runs_its_tests() {
        let dir = project(&[("Cargo.toml", "[package]\nname = \"x\"\n")]);
        assert_eq!(command(&dir), Some(Command::new("cargo test")));
        std::fs::remove_dir_all(dir).ok();
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
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_makefile_target_is_a_rule_and_not_a_variable() {
        let rules = project(&[("Makefile", "check:\n\tcargo xtask ci\n")]);
        assert_eq!(command(&rules), Some(Command::new("make check")));
        let variables = project(&[("Makefile", "check := 1\nall:\n\techo hi\n")]);
        assert_eq!(command(&variables), None);
        std::fs::remove_dir_all(rules).ok();
        std::fs::remove_dir_all(variables).ok();
    }

    #[test]
    fn an_empty_folder_has_nothing_to_run() {
        let dir = project(&[]);
        assert_eq!(command(&dir), None);
        std::fs::remove_dir_all(dir).ok();
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
        std::fs::remove_dir_all(dir).ok();
    }
}
