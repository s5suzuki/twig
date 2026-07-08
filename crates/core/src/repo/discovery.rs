use std::path::{Path, PathBuf};

use git2::{Repository, SubmoduleIgnore, SubmoduleStatus};

pub struct RepoNode {
    pub name: String,
    pub path: PathBuf,
    pub children: Vec<RepoNode>,
    pub expanded: bool,
    pub initialized: bool,
    pub dirty: bool,
    pub drifted: bool,
}

pub fn discover(path: &Path) -> Result<RepoNode, git2::Error> {
    let repo = Repository::open(path)?;
    let name = dir_name(path);

    let mut children = Vec::new();
    if let Ok(subs) = repo.submodules() {
        for sm in subs {
            let sm_name = sm.name().unwrap_or("<submodule>").to_string();
            let sm_path = path.join(sm.path());
            let (dirty, drifted) = submodule_flags(&repo, &sm_name);
            match discover(&sm_path) {
                Ok(mut node) => {
                    node.name = sm_name;
                    node.dirty = dirty;
                    node.drifted = drifted;
                    children.push(node);
                }
                Err(_) => children.push(RepoNode {
                    name: sm_name,
                    path: sm_path,
                    children: Vec::new(),
                    expanded: false,
                    initialized: false,
                    dirty,
                    drifted,
                }),
            }
        }
    }

    Ok(RepoNode {
        name,
        path: path.to_path_buf(),
        children,
        expanded: true,
        initialized: true,
        dirty: false,
        drifted: false,
    })
}

pub fn find_submodule_parent(node: &RepoNode, target: &Path) -> Option<(PathBuf, String)> {
    for child in &node.children {
        if child.path == target {
            return Some((node.path.clone(), child.name.clone()));
        }
        if let Some(found) = find_submodule_parent(child, target) {
            return Some(found);
        }
    }
    None
}

pub fn refresh_badges(node: &mut RepoNode) {
    let repo = Repository::open(&node.path).ok();
    for child in &mut node.children {
        if let Some(repo) = &repo {
            let (dirty, drifted) = submodule_flags(repo, &child.name);
            child.dirty = dirty;
            child.drifted = drifted;
        }
        let now_initialized = Repository::open(&child.path).is_ok();
        if now_initialized != child.initialized {
            rediscover_submodule(child);
        } else {
            refresh_badges(child);
        }
    }
}

fn rediscover_submodule(child: &mut RepoNode) {
    match discover(&child.path) {
        Ok(mut fresh) => {
            fresh.name = std::mem::take(&mut child.name);
            fresh.expanded = child.expanded;
            fresh.dirty = child.dirty;
            fresh.drifted = child.drifted;
            *child = fresh;
        }
        Err(_) => {
            child.initialized = false;
            child.children.clear();
        }
    }
}

fn submodule_flags(parent: &Repository, name: &str) -> (bool, bool) {
    match parent.submodule_status(name, SubmoduleIgnore::None) {
        Ok(s) => {
            let dirty = s.intersects(
                SubmoduleStatus::WD_WD_MODIFIED
                    | SubmoduleStatus::WD_INDEX_MODIFIED
                    | SubmoduleStatus::WD_UNTRACKED,
            );
            let drifted =
                s.intersects(SubmoduleStatus::WD_MODIFIED | SubmoduleStatus::INDEX_MODIFIED);
            (dirty, drifted)
        }
        Err(_) => (false, false),
    }
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
