use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

pub struct FileNode {
    pub name: String,
    pub rel: String,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
}

pub fn list_files(root: &Path, skip: &[PathBuf]) -> Vec<FileNode> {
    let skip: Vec<String> = skip
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let base = root.to_path_buf();
    let pruned = move |path: &Path| -> bool {
        let Ok(rel) = path.strip_prefix(&base) else {
            return false;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        rel == ".git"
            || rel.starts_with(".git/")
            || skip
                .iter()
                .any(|s| rel == *s || rel.starts_with(&format!("{s}/")))
    };

    let mut files: Vec<String> = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(move |e| !pruned(e.path()))
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    build_tree(&files)
}

#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: Vec<String>,
}

fn build_tree(files: &[String]) -> Vec<FileNode> {
    let mut root = Dir::default();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        let (dirs, name) = parts.split_at(parts.len() - 1);
        let mut cur = &mut root;
        for d in dirs {
            cur = cur.dirs.entry((*d).to_string()).or_default();
        }
        cur.files.push(name[0].to_string());
    }
    convert(&root, "")
}

fn convert(dir: &Dir, prefix: &str) -> Vec<FileNode> {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        }
    };
    let mut out = Vec::new();
    for (name, sub) in &dir.dirs {
        let rel = join(name);
        let children = convert(sub, &rel);
        out.push(FileNode {
            name: name.clone(),
            rel,
            is_dir: true,
            children,
        });
    }
    let mut names: Vec<&String> = dir.files.iter().collect();
    names.sort();
    for name in names {
        out.push(FileNode {
            name: name.clone(),
            rel: join(name),
            is_dir: false,
            children: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_groups_dirs_first_then_files() {
        let files = vec![
            "b.txt".to_string(),
            "a/x.rs".to_string(),
            "a/b/y.rs".to_string(),
        ];
        let t = build_tree(&files);
        assert_eq!(t.len(), 2);
        assert!(t[0].is_dir && t[0].name == "a");
        assert!(!t[1].is_dir && t[1].name == "b.txt");

        let a = &t[0];
        assert_eq!(a.children[0].name, "b");
        assert_eq!(a.children[0].rel, "a/b");
        assert!(a.children[0].is_dir);
        assert_eq!(a.children[1].name, "x.rs");
        assert_eq!(a.children[1].rel, "a/x.rs");
        assert!(!a.children[1].is_dir);
    }

    #[test]
    fn walk_skips_git_and_gitignored_keeps_dotfiles() {
        let tmp = std::env::temp_dir().join(format!("twig-files-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("target")).unwrap();
        std::fs::write(tmp.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(tmp.join("src/main.rs"), "").unwrap();
        std::fs::write(tmp.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(tmp.join("target/out.o"), "").unwrap();

        let top = list_files(&tmp, &[]);
        let names: Vec<&str> = top.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.contains(&".gitignore"),
            "dotfiles must show: {names:?}"
        );
        assert!(names.contains(&"src"));
        assert!(!names.contains(&".git"), "the .git dir must be pruned");
        assert!(!names.contains(&"target"), ".gitignore must be respected");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
