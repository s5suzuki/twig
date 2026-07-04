use std::collections::HashSet;
use std::path::Path;

use git2::{Repository, Status, StatusOptions};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    New,
    Modified,
    Deleted,
    Renamed,
    Typechange,
    Conflicted,
    Submodule,
    Other,
}

impl StatusKind {
    pub fn marker(self) -> char {
        match self {
            StatusKind::New => 'A',
            StatusKind::Modified => 'M',
            StatusKind::Deleted => 'D',
            StatusKind::Renamed => 'R',
            StatusKind::Typechange => 'T',
            StatusKind::Conflicted => 'U',
            StatusKind::Submodule => 'S',
            StatusKind::Other => '?',
        }
    }
}

pub struct StatusEntry {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: StatusKind,
}

pub fn load_status(path: &Path) -> Result<(Vec<StatusEntry>, Vec<StatusEntry>), git2::Error> {
    let repo = Repository::open(path)?;
    collect_status(&repo)
}

fn collect_status(repo: &Repository) -> Result<(Vec<StatusEntry>, Vec<StatusEntry>), git2::Error> {
    let submodules = submodule_paths(repo);

    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repo.statuses(Some(&mut opts))?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("").to_string();
        let s = entry.status();
        let is_sub = submodules.contains(path.trim_end_matches('/'));

        if s.contains(Status::CONFLICTED) {
            unstaged.push(StatusEntry {
                path,
                old_path: None,
                kind: StatusKind::Conflicted,
            });
            continue;
        }

        if let Some(kind) = index_kind(s) {
            let old_path = rename_old_path(kind, entry.head_to_index());
            staged.push(StatusEntry {
                path: delta_new_path(entry.head_to_index(), &path),
                old_path,
                kind: if is_sub { StatusKind::Submodule } else { kind },
            });
        }
        if let Some(kind) = worktree_kind(s) {
            let old_path = rename_old_path(kind, entry.index_to_workdir());
            unstaged.push(StatusEntry {
                path: delta_new_path(entry.index_to_workdir(), &path),
                old_path,
                kind: if is_sub { StatusKind::Submodule } else { kind },
            });
        }
    }
    Ok((staged, unstaged))
}

fn rename_old_path(kind: StatusKind, delta: Option<git2::DiffDelta>) -> Option<String> {
    if kind != StatusKind::Renamed {
        return None;
    }
    delta.and_then(|d| {
        d.old_file()
            .path()
            .map(|p| p.to_string_lossy().into_owned())
    })
}

fn delta_new_path(delta: Option<git2::DiffDelta>, fallback: &str) -> String {
    delta
        .and_then(|d| {
            d.new_file()
                .path()
                .or_else(|| d.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn submodule_paths(repo: &Repository) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Ok(subs) = repo.submodules() {
        for sm in subs {
            set.insert(sm.path().to_string_lossy().into_owned());
        }
    }
    set
}

fn index_kind(s: Status) -> Option<StatusKind> {
    if s.contains(Status::INDEX_RENAMED) {
        Some(StatusKind::Renamed)
    } else if s.contains(Status::INDEX_NEW) {
        Some(StatusKind::New)
    } else if s.contains(Status::INDEX_MODIFIED) {
        Some(StatusKind::Modified)
    } else if s.contains(Status::INDEX_DELETED) {
        Some(StatusKind::Deleted)
    } else if s.contains(Status::INDEX_TYPECHANGE) {
        Some(StatusKind::Typechange)
    } else {
        None
    }
}

fn worktree_kind(s: Status) -> Option<StatusKind> {
    if s.contains(Status::WT_RENAMED) {
        Some(StatusKind::Renamed)
    } else if s.contains(Status::WT_NEW) {
        Some(StatusKind::New)
    } else if s.contains(Status::WT_MODIFIED) {
        Some(StatusKind::Modified)
    } else if s.contains(Status::WT_DELETED) {
        Some(StatusKind::Deleted)
    } else if s.contains(Status::WT_TYPECHANGE) {
        Some(StatusKind::Typechange)
    } else {
        None
    }
}
