use std::path::Path;
use std::process::Command;

use twit_core::git2::{Oid, Repository};
use twit_core::repo::{self, SeqOutcome};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

fn init(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "T"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn head_oid(dir: &Path) -> Oid {
    Repository::open(dir)
        .unwrap()
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .id()
}

#[test]
fn merge_non_ff_creates_merge_commit() {
    let tmp = tempdir("non-ff");
    let dir = tmp.path();
    init(dir);
    write(dir, "base.txt", "base\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-qb", "feature"]);
    write(dir, "feature.txt", "feat\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "feat"]);
    let feature = head_oid(dir);

    git(dir, &["checkout", "-q", "main"]);
    write(dir, "main.txt", "main\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "main-work"]);

    let out = repo::merge(dir, feature, "branch 'feature'").unwrap();
    assert!(matches!(out, SeqOutcome::Done));

    let repo = Repository::open(dir).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "should be a merge commit");
    assert_eq!(head.message().unwrap().trim(), "Merge branch 'feature'");
    assert!(dir.join("feature.txt").exists());
    assert!(repo::seq_state(dir) == repo::SeqState::None);
}

#[test]
fn merge_fast_forward_moves_head() {
    let tmp = tempdir("ff");
    let dir = tmp.path();
    init(dir);
    write(dir, "base.txt", "base\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-qb", "feature"]);
    write(dir, "feature.txt", "feat\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "feat"]);
    let feature = head_oid(dir);

    git(dir, &["checkout", "-q", "main"]);
    let out = repo::merge(dir, feature, "branch 'feature'").unwrap();
    assert!(matches!(out, SeqOutcome::Done));

    let head = head_oid(dir);
    assert_eq!(head, feature, "fast-forward should move HEAD onto target");
    assert_eq!(
        Repository::open(dir)
            .unwrap()
            .head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .parent_count(),
        1,
        "fast-forward must not create a merge commit"
    );
}

#[test]
fn merge_conflict_reports_files_and_aborts() {
    let tmp = tempdir("conflict");
    let dir = tmp.path();
    init(dir);
    write(dir, "shared.txt", "base\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-qb", "feature"]);
    write(dir, "shared.txt", "feature side\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "feat"]);
    let feature = head_oid(dir);

    git(dir, &["checkout", "-q", "main"]);
    write(dir, "shared.txt", "main side\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "main-work"]);
    let before = head_oid(dir);

    let out = repo::merge(dir, feature, "branch 'feature'").unwrap();
    match out {
        SeqOutcome::Conflicts(files) => assert_eq!(files, vec!["shared.txt".to_string()]),
        SeqOutcome::Done => panic!("expected conflicts"),
    }
    assert!(repo::seq_state(dir) == repo::SeqState::Merge);

    repo::merge_abort(dir).unwrap();
    assert_eq!(head_oid(dir), before, "abort restores HEAD");
    assert!(repo::seq_state(dir) == repo::SeqState::None);
}

fn tempdir(name: &str) -> TempDir {
    let base = std::env::temp_dir().join(format!("twit-merge-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    TempDir { path: base }
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
