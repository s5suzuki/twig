use std::path::Path;
use std::process::Command;

use twit_core::repo::{self, DiffMode};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_ALLOW_PROTOCOL", "file")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn init(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "T"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "protocol.file.allow", "always"]);
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

fn tempdir(name: &str) -> TempDir {
    let base = std::env::temp_dir().join(format!("twit-sub-disc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    TempDir { path: base }
}

fn setup(name: &str) -> (TempDir, std::path::PathBuf, String) {
    let tmp = tempdir(name);
    let root = tmp.path().to_path_buf();

    let origin = root.join("sub_origin");
    std::fs::create_dir_all(&origin).unwrap();
    init(&origin);
    std::fs::write(origin.join("f.txt"), "v1\n").unwrap();
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-qm", "c1"]);
    std::fs::write(origin.join("f.txt"), "v2\n").unwrap();
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-qm", "c2"]);

    let parent = root.join("parent");
    std::fs::create_dir_all(&parent).unwrap();
    init(&parent);
    git(&parent, &["submodule", "add", "-q", "../sub_origin", "sub"]);
    git(&parent, &["commit", "-qm", "add submodule at c2"]);

    let sub = parent.join("sub");
    let recorded = git_out(&sub, &["rev-parse", "HEAD"]);
    git(&sub, &["checkout", "-q", "HEAD~1"]);
    assert_ne!(git_out(&sub, &["rev-parse", "HEAD"]), recorded);
    assert!(git_out(&parent, &["status", "--porcelain"]).contains("sub"));

    (tmp, parent, recorded)
}

#[test]
fn file_discard_resets_submodule_head() {
    let (_tmp, parent, recorded) = setup("file");
    repo::discard(&parent, &["sub".to_string()]).unwrap();
    assert_eq!(git_out(&parent, &["status", "--porcelain"]), "");
    assert_eq!(
        git_out(&parent.join("sub"), &["rev-parse", "HEAD"]),
        recorded
    );
}

#[test]
fn diff_partial_discard_resets_submodule_head() {
    let (_tmp, parent, recorded) = setup("partial");
    let d = repo::file_diff(&parent, "sub", DiffMode::Unstaged).unwrap();
    let n = d.rows.len();
    repo::discard_partial(&parent, "sub", &d.rows, 0, n.saturating_sub(1)).unwrap();
    assert_eq!(git_out(&parent, &["status", "--porcelain"]), "");
    assert_eq!(
        git_out(&parent.join("sub"), &["rev-parse", "HEAD"]),
        recorded
    );
}
