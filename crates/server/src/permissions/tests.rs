//! The rules' answers to the commands, paths and addresses an agent can hide behind.

// Rust guideline compliant 2026-10-09

use std::path::Path;

use super::command::split_command;
use super::matching::host;
use super::{Effect, PermissionKind, PermissionRule, PermissionSettings, Target, decide, seed};

/// The starting rules: commands that only read are allowed, and `rm`, `sudo` and the like ask.
fn defaults() -> PermissionSettings {
    let mut settings = PermissionSettings::default();
    seed(&mut settings);
    settings
}

/// Whether the starting rules let `line` run without asking, in a project at `/project`.
fn runs_unasked(line: &str) -> bool {
    let target = Target::new(PermissionKind::Command, [line]);
    decide(&defaults(), None, Path::new("/project"), &target, &[]).is_some_and(|d| d.allow)
}

/// The commands `line` is read as.
fn commands(line: &str) -> Vec<String> {
    split_command(line)
        .0
        .iter()
        .map(|words| words.join(" "))
        .collect()
}

#[test]
fn commands_that_only_read_run_unasked() {
    for line in [
        "ls -la",
        "git diff --stat",
        "cat a.txt | head -n 5",
        "bash -lc 'ls && git status'",
        "rg needle src 2>/dev/null",
    ] {
        assert!(runs_unasked(line), "{line}");
    }
}

#[test]
fn a_comment_does_not_hide_the_line_after_it() {
    assert_eq!(commands("ls # it's\nrm -rf x"), ["ls", "rm -rf x"]);
    assert!(!runs_unasked("ls # it's\nrm -rf x"));
    // Inside a word, `#` is part of it.
    assert_eq!(commands("echo a#b"), ["echo a#b"]);
}

#[test]
fn a_quote_that_never_closes_is_not_allowed() {
    assert!(!split_command("ls 'never closed").1);
    assert!(!runs_unasked("ls \"never closed"));
}

#[test]
fn a_rule_sees_the_command_a_wrapper_or_group_runs() {
    assert_eq!(commands("(rm x)"), ["rm x"]);
    assert_eq!(commands("{ rm x; }"), ["rm x"]);
    assert_eq!(commands("echo $(rm x)"), ["echo $", "rm x"]);
    assert_eq!(commands("bash -c 'rm x' _"), ["rm x"]);
    assert_eq!(commands("r\\\nm x"), ["rm x"]);
    assert!(commands("env FOO=1 rm x").contains(&"rm x".to_string()));
    assert!(commands("xargs rm").contains(&"rm".to_string()));
    assert!(commands("nice -n 5 rm x").contains(&"rm x".to_string()));
    assert!(commands("sudo env bash -ec 'rm y'").contains(&"rm y".to_string()));
    assert!(commands("bash -o pipefail -c 'rm z'").contains(&"rm z".to_string()));
}

#[test]
fn an_allowed_name_does_not_allow_another_program() {
    // A file in the project named after an allowed command, and a command whose program an
    // assignment can swap.
    assert!(!runs_unasked("./ls"));
    assert!(!runs_unasked("PATH=. ls"));
    assert!(!runs_unasked("LD_PRELOAD=./x.so cat a.txt"));
}

#[test]
fn an_allowed_reader_may_not_run_or_write() {
    assert!(!runs_unasked("rg --pre ./run.sh needle"));
    assert!(!runs_unasked("rg --pre=./run.sh needle"));
    assert!(runs_unasked("rg --pre-glob '*.pdf' needle"));
    assert!(!runs_unasked("git diff --output=/home/me/.bashrc"));
    assert!(!runs_unasked("git log --outp x"));
    assert!(!runs_unasked("tree -o out.txt"));
}

#[test]
fn a_rule_that_asks_covers_the_program_by_its_path() {
    let mut settings = PermissionSettings::default();
    settings.everywhere.rules.push(PermissionRule {
        id: "deny-rm".into(),
        kind: PermissionKind::Command,
        pattern: "rm".into(),
        effect: Effect::Deny,
    });
    let target = Target::new(PermissionKind::Command, ["/bin/rm -rf x"]);
    let decision = decide(&settings, None, Path::new("/project"), &target, &[]);
    assert_eq!(decision.map(|d| d.allow), Some(false));
}

#[test]
fn an_address_clients_read_two_ways_names_no_host() {
    assert_eq!(host("https://docs.rs/serde"), "docs.rs");
    assert_eq!(host("https://user@Example.com:8080/x"), "example.com");
    assert_eq!(host("https://evil.example\\@allowed.example/"), "");
    assert_eq!(host("https://allowed.example /x"), "");
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_project_is_outside_it() {
    use crate::system::fs::is_inside;
    let dir = std::env::temp_dir().join(crate::system::new_id());
    let (project, outside) = (dir.join("project"), dir.join("outside"));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret"), "x").unwrap();
    std::os::unix::fs::symlink(&outside, project.join("link")).unwrap();
    assert!(is_inside(&project, "src/new.rs"));
    assert!(!is_inside(&project, "link/secret"));
    assert!(!is_inside(&project, "link/not-there-yet"));
    assert!(!is_inside(&project, "../outside/secret"));
    // An allow rule for the project does not reach through the link.
    let mut settings = PermissionSettings::default();
    settings.everywhere.rules.push(PermissionRule {
        id: "project".into(),
        kind: PermissionKind::Edit,
        pattern: ".".into(),
        effect: Effect::Allow,
    });
    let allowed = |path: &str| {
        let target = Target::new(PermissionKind::Edit, [path]);
        decide(&settings, None, &project, &target, &[]).is_some_and(|d| d.allow)
    };
    assert!(allowed("src/new.rs"));
    assert!(!allowed("link/secret"));
    let _ = std::fs::remove_dir_all(&dir);
}
