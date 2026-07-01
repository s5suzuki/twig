use std::path::{Path, PathBuf};

use git2::Repository;

pub struct RepoNode {
    pub name: String,
    pub path: PathBuf,
    pub children: Vec<RepoNode>,
    pub expanded: bool,
    pub initialized: bool,
}

pub fn discover(path: &Path) -> Result<RepoNode, git2::Error> {
    let repo = Repository::open(path)?;
    let name = dir_name(path);

    let mut children = Vec::new();
    if let Ok(subs) = repo.submodules() {
        for sm in subs {
            let sm_name = sm.name().unwrap_or("<submodule>").to_string();
            let sm_path = path.join(sm.path());
            match discover(&sm_path) {
                Ok(mut node) => {
                    node.name = sm_name;
                    children.push(node);
                }
                Err(_) => children.push(RepoNode {
                    name: sm_name,
                    path: sm_path,
                    children: Vec::new(),
                    expanded: false,
                    initialized: false,
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
    })
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
