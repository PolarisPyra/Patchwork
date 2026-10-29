use std::path::Path;

use super::git::git;
use super::*;
use crate::system::new_id;

/// Creates a repo under the temp dir with one committed file, `a.txt`.
fn temp_repo() -> String {
    let dir = std::env::temp_dir().join(new_id());
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.to_string_lossy().into_owned();
    git(&root, &["init", "-b", "main"]).unwrap();
    git(&root, &["config", "user.name", "Test"]).unwrap();
    git(&root, &["config", "user.email", "test@example.com"]).unwrap();
    git(&root, &["config", "commit.gpgsign", "false"]).unwrap();
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    git(&root, &["add", "--", "a.txt"]).unwrap();
    git(&root, &["commit", "-q", "-m", "init"]).unwrap();
    root
}

#[test]
fn reports_modified_and_untracked_files() {
    let root = temp_repo();
    std::fs::write(Path::new(&root).join("a.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(Path::new(&root).join("b.txt"), "x\ny\n").unwrap();

    let changes = git_changes(&root).unwrap();
    assert_eq!(changes.len(), 2);
    let a = &changes[0];
    assert_eq!(
        (
            a.path.as_str(),
            a.status.as_str(),
            a.additions,
            a.deletions,
            a.staged
        ),
        ("a.txt", "modified", 1, 0, false)
    );
    let b = &changes[1];
    assert_eq!(
        (b.status.as_str(), b.additions, b.staged),
        ("untracked", 2, false)
    );

    assert!(git_file_diff(&root, "a.txt").unwrap().contains("+three"));
    assert!(!git_file_diff(&root, "b.txt").unwrap().is_empty());

    accept_file(&root, "a.txt").unwrap();
    let changes = git_changes(&root).unwrap();
    assert!(changes.iter().any(|c| c.path == "a.txt" && c.staged));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn revert_restores_tracked_and_removes_new_files() {
    let root = temp_repo();
    let dir = Path::new(&root);
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    std::fs::write(dir.join("b.txt"), "new\n").unwrap();
    std::fs::write(dir.join("c.txt"), "staged\n").unwrap();
    git(&root, &["add", "--", "c.txt"]).unwrap();

    revert_file(&root, "a.txt").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    revert_file(&root, "b.txt").unwrap();
    assert!(!dir.join("b.txt").exists());
    revert_file(&root, "c.txt").unwrap();
    assert!(!dir.join("c.txt").exists());
    assert!(git_changes(&root).unwrap().is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn read_file_stays_inside_project() {
    let root = temp_repo();
    let outside = Path::new(&root)
        .parent()
        .unwrap()
        .join(format!("{}.txt", new_id()));
    std::fs::write(&outside, "secret").unwrap();
    let escape = format!("../{}", outside.file_name().unwrap().to_string_lossy());

    assert!(
        read_file(&root, &escape)
            .unwrap_err()
            .to_string()
            .contains("outside")
    );
    assert_eq!(read_file(&root, "a.txt").unwrap(), "one\ntwo\n");
    std::fs::remove_file(&outside).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}
