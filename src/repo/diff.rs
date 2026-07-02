use std::ops::Range;
use std::path::Path;

use git2::{
    ApplyLocation, ApplyOptions, Diff, DiffFindOptions, DiffOptions, Oid, Patch, Repository,
};

use super::status::StatusKind;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    Staged,
    Unstaged,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    Changed,
}

#[derive(Hash)]
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
        left_emph: Vec<Range<usize>>,
        right_emph: Vec<Range<usize>>,
    },
}

pub struct FileDiff {
    pub rows: Vec<DiffRow>,
    pub note: Option<String>,

    pub conflict: bool,
    pub rename: bool,
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

fn renamed_old_path(
    repo: &Repository,
    mode: DiffMode,
    file: &str,
) -> Result<Option<String>, git2::Error> {
    let mut opts = DiffOptions::new();
    let mut diff = make_diff(repo, mode, &mut opts)?;
    let mut find = DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find))?;

    for delta in diff.deltas() {
        if delta.status() != git2::Delta::Renamed {
            continue;
        }
        let new_p = delta.new_file().path().map(|p| p.to_string_lossy().into_owned());
        if new_p.as_deref() != Some(file) {
            continue;
        }
        let old_p = delta.old_file().path().map(|p| p.to_string_lossy().into_owned());
        return Ok(old_p.filter(|o| o != file));
    }
    Ok(None)
}

pub fn file_diff(repo_path: &Path, file: &str, mode: DiffMode) -> Result<FileDiff, git2::Error> {
    let repo = Repository::open(repo_path)?;

    if let Some(d) = conflict_file_diff(&repo, file)? {
        return Ok(d);
    }

    let old = renamed_old_path(&repo, mode, file)?;
    let scoped = |opts: &mut DiffOptions| {
        opts.pathspec(file);
        if let Some(o) = &old {
            opts.pathspec(o);
        }
    };
    let find_renames = |diff: &mut Diff| -> Result<(), git2::Error> {
        if old.is_some() {
            let mut find = DiffFindOptions::new();
            find.renames(true);
            diff.find_similar(Some(&mut find))?;
        }
        Ok(())
    };

    let headers = {
        let mut opts = DiffOptions::new();
        scoped(&mut opts);
        opts.context_lines(0);
        let mut zero = make_diff(&repo, mode, &mut opts)?;
        find_renames(&mut zero)?;
        hunk_headers(&zero)?
    };

    let mut opts = DiffOptions::new();
    scoped(&mut opts);
    opts.context_lines(1_000_000_000);
    let mut diff = make_diff(&repo, mode, &mut opts)?;
    find_renames(&mut diff)?;

    let mut rows = Vec::new();
    if let Some(o) = &old {
        rows.push(DiffRow::FileHeader(format!("{o}  →  {file}")));
    }
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
                            left_emph: Vec::new(),
                            right_emph: Vec::new(),
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
        rename: old.is_some(),
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
        rename: false,
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
        rename: false,
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
        rename: false,
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
                        left_emph: Vec::new(),
                        right_emph: Vec::new(),
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
        let (left_emph, right_emph) = match (kind, left, right) {
            (LineKind::Changed, Some((_, l)), Some((_, r))) => intra_emphasis(l, r),
            _ => (Vec::new(), Vec::new()),
        };
        rows.push(DiffRow::Line {
            old_no: left.and_then(|(n, _)| *n),
            new_no: right.and_then(|(n, _)| *n),
            left: left.map(|(_, s)| s.clone()),
            right: right.map(|(_, s)| s.clone()),
            kind,
            left_emph,
            right_emph,
        });
    }
    dels.clear();
    adds.clear();
}

const EMPH_MAX_LEN: usize = 1000;

fn intra_emphasis(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if old.len() > EMPH_MAX_LEN || new.len() > EMPH_MAX_LEN {
        return (Vec::new(), Vec::new());
    }
    let ob = old.as_bytes();
    let nb = new.as_bytes();

    let mut p = 0;
    let max_p = ob.len().min(nb.len());
    while p < max_p && ob[p] == nb[p] {
        p += 1;
    }
    while p > 0 && !(old.is_char_boundary(p) && new.is_char_boundary(p)) {
        p -= 1;
    }

    let mut s = 0;
    let max_s = (ob.len() - p).min(nb.len() - p);
    while s < max_s && ob[ob.len() - 1 - s] == nb[nb.len() - 1 - s] {
        s += 1;
    }
    let mut old_end = ob.len() - s;
    let mut new_end = nb.len() - s;
    while old_end < ob.len() && new_end < nb.len() && !old.is_char_boundary(old_end) {
        old_end += 1;
        new_end += 1;
    }

    let total = ob.len().max(nb.len());
    let common = p + (ob.len() - old_end);
    if total == 0 || (common as f64) < 0.5 * (total as f64) {
        return (Vec::new(), Vec::new());
    }

    let left = if p < old_end { vec![p..old_end] } else { Vec::new() };
    let right = if p < new_end { vec![p..new_end] } else { Vec::new() };
    (left, right)
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
