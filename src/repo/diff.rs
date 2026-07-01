use std::path::Path;

use git2::{ApplyLocation, ApplyOptions, Diff, DiffOptions, Oid, Patch, Repository};

use super::status::StatusKind;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    Staged,
    Unstaged,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    Changed,
}

pub enum DiffRow {
    FileHeader(String),
    Hunk {
        index: usize,
        header: String,
    },
    Line {
        old_no: Option<u32>,
        new_no: Option<u32>,
        left: Option<String>,
        right: Option<String>,
        kind: LineKind,
    },
}

pub struct FileDiff {
    pub rows: Vec<DiffRow>,
    pub note: Option<String>,

    pub conflict: bool,
}

pub struct CommitFile {
    pub path: String,
    pub kind: StatusKind,
}

fn make_diff<'a>(
    repo: &'a Repository,
    mode: DiffMode,
    opts: &mut DiffOptions,
) -> Result<Diff<'a>, git2::Error> {
    match mode {
        DiffMode::Unstaged => {
            opts.include_untracked(true);
            opts.recurse_untracked_dirs(true);
            opts.show_untracked_content(true);
            repo.diff_index_to_workdir(None, Some(opts))
        }
        DiffMode::Staged => {
            let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
            repo.diff_tree_to_index(head_tree.as_ref(), None, Some(opts))
        }
    }
}

fn hunk_headers(diff: &Diff) -> Result<Vec<String>, git2::Error> {
    let mut out = Vec::new();
    for idx in 0..diff.deltas().len() {
        let Some(patch) = Patch::from_diff(diff, idx)? else {
            continue;
        };
        for h in 0..patch.num_hunks() {
            let (hunk, _) = patch.hunk(h)?;
            out.push(
                String::from_utf8_lossy(hunk.header())
                    .trim_end()
                    .to_string(),
            );
        }
    }
    Ok(out)
}

pub fn file_diff(repo_path: &Path, file: &str, mode: DiffMode) -> Result<FileDiff, git2::Error> {
    let repo = Repository::open(repo_path)?;

    if let Some(d) = conflict_file_diff(&repo, file)? {
        return Ok(d);
    }

    let headers = {
        let mut opts = DiffOptions::new();
        opts.pathspec(file);
        opts.context_lines(0);
        let zero = make_diff(&repo, mode, &mut opts)?;
        hunk_headers(&zero)?
    };

    let mut opts = DiffOptions::new();
    opts.pathspec(file);
    opts.context_lines(1_000_000_000);
    let diff = make_diff(&repo, mode, &mut opts)?;

    let mut rows = Vec::new();
    let mut block = 0usize;
    for idx in 0..diff.deltas().len() {
        let Some(patch) = Patch::from_diff(&diff, idx)? else {
            continue;
        };
        for h in 0..patch.num_hunks() {
            let (_hunk, line_count) = patch.hunk(h)?;

            let mut dels: Vec<(Option<u32>, String)> = Vec::new();
            let mut adds: Vec<(Option<u32>, String)> = Vec::new();

            for l in 0..line_count {
                let line = patch.line_in_hunk(h, l)?;
                let text = String::from_utf8_lossy(line.content())
                    .trim_end_matches('\n')
                    .to_string();
                match line.origin() {
                    '-' => dels.push((line.old_lineno(), text)),
                    '+' => adds.push((line.new_lineno(), text)),
                    _ => {
                        flush_block(&mut rows, &mut dels, &mut adds, &mut block, &headers);
                        rows.push(DiffRow::Line {
                            old_no: line.old_lineno(),
                            new_no: line.new_lineno(),
                            left: Some(text.clone()),
                            right: Some(text),
                            kind: LineKind::Context,
                        });
                    }
                }
            }
            flush_block(&mut rows, &mut dels, &mut adds, &mut block, &headers);
        }
    }

    let note = if rows.is_empty() {
        Some("(no changes, or binary)".to_string())
    } else {
        None
    };
    Ok(FileDiff {
        rows,
        note,
        conflict: false,
    })
}

fn conflict_file_diff(repo: &Repository, file: &str) -> Result<Option<FileDiff>, git2::Error> {
    let index = repo.index()?;
    let path = Path::new(file);
    let ours = index.get_path(path, 2);
    let theirs = index.get_path(path, 3);
    if ours.is_none() && theirs.is_none() {
        return Ok(None);
    }

    let content = |entry: Option<git2::IndexEntry>| -> Vec<u8> {
        entry
            .and_then(|e| repo.find_blob(e.id).ok())
            .map(|b| b.content().to_vec())
            .unwrap_or_default()
    };
    let ours_buf = content(ours);
    let theirs_buf = content(theirs);

    let mut opts = DiffOptions::new();
    opts.context_lines(1_000_000_000);
    let patch = Patch::from_buffers(
        &ours_buf,
        Some(path),
        &theirs_buf,
        Some(path),
        Some(&mut opts),
    )?;

    let mut rows = vec![DiffRow::FileHeader(format!(
        "{file}  —  conflict (◀ ours · theirs ▶)"
    ))];
    let mut block = 0usize;
    append_patch_rows(&patch, &mut rows, &mut block)?;

    Ok(Some(FileDiff {
        rows,
        note: None,
        conflict: true,
    }))
}

fn delta_kind(d: git2::Delta) -> StatusKind {
    use git2::Delta;
    match d {
        Delta::Added => StatusKind::New,
        Delta::Deleted => StatusKind::Deleted,
        Delta::Modified => StatusKind::Modified,
        Delta::Renamed => StatusKind::Renamed,
        Delta::Copied => StatusKind::New,
        Delta::Typechange => StatusKind::Typechange,
        _ => StatusKind::Other,
    }
}

pub fn commit_files(repo_path: &Path, oid: Oid) -> Result<Vec<CommitFile>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

    let mut out = Vec::new();
    for delta in diff.deltas() {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(CommitFile {
            path,
            kind: delta_kind(delta.status()),
        });
    }
    Ok(out)
}

pub fn commit_message(repo_path: &Path, oid: Oid) -> Result<String, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    Ok(commit.message().unwrap_or("").to_string())
}

pub fn commit_file_diff(repo_path: &Path, oid: Oid, file: &str) -> Result<FileDiff, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };

    let mut opts = DiffOptions::new();
    opts.pathspec(file);
    opts.context_lines(3);
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts))?;

    let mut rows = Vec::new();
    let mut block = 0usize;
    for idx in 0..diff.deltas().len() {
        let Some(patch) = Patch::from_diff(&diff, idx)? else {
            continue;
        };
        append_patch_rows(&patch, &mut rows, &mut block)?;
    }

    let note = if rows.is_empty() {
        Some("(no changes in this file)".to_string())
    } else {
        None
    };
    Ok(FileDiff {
        rows,
        note,
        conflict: false,
    })
}

pub fn commit_diff(repo_path: &Path, oid: Oid) -> Result<FileDiff, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };

    let mut opts = DiffOptions::new();
    opts.context_lines(3);
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts))?;

    let mut rows = Vec::new();
    let mut block = 0usize;
    for idx in 0..diff.deltas().len() {
        let delta = diff
            .get_delta(idx)
            .expect("delta index within deltas() range");
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        let Some(patch) = Patch::from_diff(&diff, idx)? else {
            rows.push(DiffRow::FileHeader(format!("{path}  (binary)")));
            continue;
        };
        rows.push(DiffRow::FileHeader(path));
        append_patch_rows(&patch, &mut rows, &mut block)?;
    }

    let note = if rows.is_empty() {
        Some("(no changes in this commit)".to_string())
    } else {
        None
    };
    Ok(FileDiff {
        rows,
        note,
        conflict: false,
    })
}

fn append_patch_rows(
    patch: &Patch,
    rows: &mut Vec<DiffRow>,
    block: &mut usize,
) -> Result<(), git2::Error> {
    for h in 0..patch.num_hunks() {
        let (hunk, line_count) = patch.hunk(h)?;
        rows.push(DiffRow::Hunk {
            index: *block,
            header: String::from_utf8_lossy(hunk.header())
                .trim_end()
                .to_string(),
        });
        *block += 1;

        let mut dels: Vec<(Option<u32>, String)> = Vec::new();
        let mut adds: Vec<(Option<u32>, String)> = Vec::new();
        for l in 0..line_count {
            let line = patch.line_in_hunk(h, l)?;
            let text = String::from_utf8_lossy(line.content())
                .trim_end_matches('\n')
                .to_string();
            match line.origin() {
                '-' => dels.push((line.old_lineno(), text)),
                '+' => adds.push((line.new_lineno(), text)),
                _ => {
                    flush_pairs(rows, &mut dels, &mut adds);
                    rows.push(DiffRow::Line {
                        old_no: line.old_lineno(),
                        new_no: line.new_lineno(),
                        left: Some(text.clone()),
                        right: Some(text),
                        kind: LineKind::Context,
                    });
                }
            }
        }
        flush_pairs(rows, &mut dels, &mut adds);
    }
    Ok(())
}

fn flush_block(
    rows: &mut Vec<DiffRow>,
    dels: &mut Vec<(Option<u32>, String)>,
    adds: &mut Vec<(Option<u32>, String)>,
    block: &mut usize,
    headers: &[String],
) {
    if dels.is_empty() && adds.is_empty() {
        return;
    }
    let idx = *block;
    let header = headers
        .get(idx)
        .cloned()
        .unwrap_or_else(|| format!("@@ hunk {} @@", idx + 1));
    rows.push(DiffRow::Hunk { index: idx, header });
    *block += 1;
    flush_pairs(rows, dels, adds);
}

fn flush_pairs(
    rows: &mut Vec<DiffRow>,
    dels: &mut Vec<(Option<u32>, String)>,
    adds: &mut Vec<(Option<u32>, String)>,
) {
    let n = dels.len().max(adds.len());
    for i in 0..n {
        let left = dels.get(i);
        let right = adds.get(i);
        let kind = match (left.is_some(), right.is_some()) {
            (true, true) => LineKind::Changed,
            (true, false) => LineKind::Removed,
            _ => LineKind::Added,
        };
        rows.push(DiffRow::Line {
            old_no: left.and_then(|(n, _)| *n),
            new_no: right.and_then(|(n, _)| *n),
            left: left.map(|(_, s)| s.clone()),
            right: right.map(|(_, s)| s.clone()),
            kind,
        });
    }
    dels.clear();
    adds.clear();
}

pub fn build_partial_patch(
    file: &str,
    rows: &[DiffRow],
    lo: usize,
    hi: usize,
    unstage: bool,
) -> Option<String> {
    let mut body = String::new();
    let mut old_count = 0u32;
    let mut new_count = 0u32;
    let mut changed = false;

    for (i, row) in rows.iter().enumerate() {
        let DiffRow::Line {
            left, right, kind, ..
        } = row
        else {
            continue;
        };

        let (base, target): (Option<&str>, Option<&str>) = if !unstage {
            match kind {
                LineKind::Context => (left.as_deref(), left.as_deref()),
                LineKind::Removed => (left.as_deref(), None),
                LineKind::Added => (None, right.as_deref()),
                LineKind::Changed => (left.as_deref(), right.as_deref()),
            }
        } else {
            match kind {
                LineKind::Context => (right.as_deref(), right.as_deref()),
                LineKind::Removed => (None, left.as_deref()),
                LineKind::Added => (right.as_deref(), None),
                LineKind::Changed => (right.as_deref(), left.as_deref()),
            }
        };

        if *kind == LineKind::Context {
            if let Some(b) = base {
                push_line(&mut body, ' ', b);
                old_count += 1;
                new_count += 1;
            }
            continue;
        }

        let selected = i >= lo && i <= hi;
        if selected {
            if let Some(b) = base {
                push_line(&mut body, '-', b);
                old_count += 1;
                changed = true;
            }
            if let Some(t) = target {
                push_line(&mut body, '+', t);
                new_count += 1;
                changed = true;
            }
        } else if let Some(b) = base {
            push_line(&mut body, ' ', b);
            old_count += 1;
            new_count += 1;
        }
    }

    if !changed {
        return None;
    }

    let old_start = if old_count == 0 { 0 } else { 1 };
    let new_start = if new_count == 0 { 0 } else { 1 };
    let mut out = format!(
        "diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -{old_start},{old_count} +{new_start},{new_count} @@\n",
    );
    out.push_str(&body);
    Some(out)
}

fn push_line(body: &mut String, origin: char, text: &str) {
    body.push(origin);
    body.push_str(text);
    body.push('\n');
}

pub fn apply_partial(
    repo_path: &Path,
    file: &str,
    rows: &[DiffRow],
    lo: usize,
    hi: usize,
    unstage: bool,
) -> Result<(), git2::Error> {
    let patch = build_partial_patch(file, rows, lo, hi, unstage)
        .ok_or_else(|| git2::Error::from_str("no lines selected"))?;
    let repo = Repository::open(repo_path)?;
    let diff = Diff::from_buffer(patch.as_bytes())?;
    let mut opts = ApplyOptions::new();
    repo.apply(&diff, ApplyLocation::Index, Some(&mut opts))
}
