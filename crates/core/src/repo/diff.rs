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
    Meta(String),
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
    pub binary: bool,
}

impl FileDiff {
    pub fn empty() -> Self {
        FileDiff {
            rows: Vec::new(),
            note: None,
            conflict: false,
            rename: false,
            binary: false,
        }
    }
}

pub fn hash_rows(rows: &[DiffRow]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rows.hash(&mut h);
    h.finish()
}

pub struct CommitFile {
    pub path: String,
    pub kind: StatusKind,
}

pub enum CommitRowKind {
    Folder {
        name: String,
        path: String,
        open: bool,
    },
    File(usize),
}

pub struct CommitFileRow {
    pub depth: usize,
    pub kind: CommitRowKind,
}

pub fn commit_file_rows(
    files: &[CommitFile],
    tree: bool,
    folded: &std::collections::HashSet<String>,
) -> Vec<CommitFileRow> {
    if !tree {
        return files
            .iter()
            .enumerate()
            .map(|(i, _)| CommitFileRow {
                depth: 0,
                kind: CommitRowKind::File(i),
            })
            .collect();
    }

    #[derive(Default)]
    struct Node {
        dirs: std::collections::BTreeMap<String, Node>,
        files: Vec<(String, usize)>,
    }
    let mut root = Node::default();
    for (i, f) in files.iter().enumerate() {
        let parts: Vec<&str> = f.path.split('/').collect();
        let (dirs, name) = parts.split_at(parts.len() - 1);
        let mut cur = &mut root;
        for d in dirs {
            cur = cur.dirs.entry((*d).to_string()).or_default();
        }
        cur.files.push((name[0].to_string(), i));
    }

    fn walk(
        node: &mut Node,
        depth: usize,
        prefix: &str,
        folded: &std::collections::HashSet<String>,
        out: &mut Vec<CommitFileRow>,
    ) {
        for (name, sub) in &mut node.dirs {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let open = !folded.contains(&path);
            out.push(CommitFileRow {
                depth,
                kind: CommitRowKind::Folder {
                    name: name.clone(),
                    path: path.clone(),
                    open,
                },
            });
            if open {
                walk(sub, depth + 1, &path, folded, out);
            }
        }
        node.files.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, idx) in &node.files {
            out.push(CommitFileRow {
                depth,
                kind: CommitRowKind::File(*idx),
            });
        }
    }
    let mut out = Vec::new();
    walk(&mut root, 0, "", folded, &mut out);
    out
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
        let new_p = delta
            .new_file()
            .path()
            .map(|p| p.to_string_lossy().into_owned());
        if new_p.as_deref() != Some(file) {
            continue;
        }
        let old_p = delta
            .old_file()
            .path()
            .map(|p| p.to_string_lossy().into_owned());
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
    let mut binary = false;
    let mut block = 0usize;
    for idx in 0..diff.deltas().len() {
        let Some(patch) = Patch::from_diff(&diff, idx)? else {
            binary = true;
            continue;
        };
        if let Some(delta) = diff.get_delta(idx) {
            binary |= delta.flags().is_binary();
        }
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

    let note = if !rows.is_empty() {
        None
    } else if binary {
        Some("(binary)".to_string())
    } else {
        Some("(no changes)".to_string())
    };
    Ok(FileDiff {
        rows,
        note,
        conflict: false,
        rename: old.is_some(),
        binary,
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
        binary: false,
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

fn stash_untracked_tree<'r>(commit: &git2::Commit<'r>) -> Option<git2::Tree<'r>> {
    if commit.parent_count() < 3 {
        return None;
    }
    let parent = commit.parent(2).ok()?;
    let summary = parent.summary().ok().flatten().unwrap_or("");
    if summary.starts_with("untracked files on ") {
        parent.tree().ok()
    } else {
        None
    }
}

fn commit_full_diff<'r>(
    repo: &'r Repository,
    commit: &git2::Commit<'r>,
    mut opts: DiffOptions,
) -> Result<Diff<'r>, git2::Error> {
    let tree = commit.tree()?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };
    let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts))?;
    if let Some(untracked) = stash_untracked_tree(commit) {
        let extra = repo.diff_tree_to_tree(None, Some(&untracked), Some(&mut opts))?;
        diff.merge(&extra)?;
    }
    Ok(diff)
}

pub fn commit_files(repo_path: &Path, oid: Oid) -> Result<Vec<CommitFile>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    let diff = commit_full_diff(&repo, &commit, DiffOptions::new())?;

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

    let mut opts = DiffOptions::new();
    opts.pathspec(file);
    opts.context_lines(3);
    let diff = commit_full_diff(&repo, &commit, opts)?;

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
        binary: false,
    })
}

pub fn commit_diff(repo_path: &Path, oid: Oid) -> Result<FileDiff, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;

    let mut opts = DiffOptions::new();
    opts.context_lines(3);
    let diff = commit_full_diff(&repo, &commit, opts)?;

    let mut rows = Vec::new();
    let message = commit.message().unwrap_or("").trim_end().to_string();
    if !message.is_empty() {
        for line in message.lines() {
            rows.push(DiffRow::Meta(line.to_string()));
        }
        rows.push(DiffRow::Meta(String::new()));
    }
    let msg_rows = rows.len();
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

    let note = if rows.len() == msg_rows {
        rows.clear();
        Some("(no changes in this commit)".to_string())
    } else {
        None
    };
    Ok(FileDiff {
        rows,
        note,
        conflict: false,
        rename: false,
        binary: false,
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

    let left = if p < old_end {
        vec![p..old_end]
    } else {
        Vec::new()
    };
    let right = if p < new_end {
        vec![p..new_end]
    } else {
        Vec::new()
    };
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

pub fn discard_partial(
    repo_path: &Path,
    file: &str,
    rows: &[DiffRow],
    lo: usize,
    hi: usize,
) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    if super::ops::reset_submodule_pointer(&repo, file)? {
        return Ok(());
    }
    let patch = build_partial_patch(file, rows, lo, hi, true)
        .ok_or_else(|| git2::Error::from_str("no lines selected"))?;
    let diff = Diff::from_buffer(patch.as_bytes())?;
    let mut opts = ApplyOptions::new();
    repo.apply(&diff, ApplyLocation::WorkDir, Some(&mut opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cf(path: &str) -> CommitFile {
        CommitFile {
            path: path.to_string(),
            kind: StatusKind::Modified,
        }
    }

    fn render(files: &[CommitFile], tree: bool, folded: &[&str]) -> Vec<String> {
        let folded: std::collections::HashSet<String> =
            folded.iter().map(|s| s.to_string()).collect();
        commit_file_rows(files, tree, &folded)
            .into_iter()
            .map(|r| match r.kind {
                CommitRowKind::Folder { name, open, .. } => {
                    format!(
                        "{}[{}{}]",
                        "  ".repeat(r.depth),
                        if open { "" } else { "+" },
                        name
                    )
                }
                CommitRowKind::File(i) => format!("{}{}", "  ".repeat(r.depth), files[i].path),
            })
            .collect()
    }

    #[test]
    fn list_mode_keeps_order() {
        let files = vec![cf("b.rs"), cf("a/x.rs"), cf("a.rs")];
        assert_eq!(render(&files, false, &[]), vec!["b.rs", "a/x.rs", "a.rs"]);
    }

    #[test]
    fn tree_mode_groups_dirs_then_files() {
        let files = vec![
            cf("src/app.rs"),
            cf("src/ui/mod.rs"),
            cf("README.md"),
            cf("src/ui/graph.rs"),
        ];
        assert_eq!(
            render(&files, true, &[]),
            vec![
                "[src]",
                "  [ui]",
                "    src/ui/graph.rs",
                "    src/ui/mod.rs",
                "  src/app.rs",
                "README.md",
            ]
        );
    }

    #[test]
    fn folded_dir_hides_children() {
        let files = vec![cf("src/app.rs"), cf("src/ui/mod.rs"), cf("src/ui/graph.rs")];
        assert_eq!(
            render(&files, true, &["src/ui"]),
            vec!["[src]", "  [+ui]", "  src/app.rs"]
        );
    }

    #[test]
    fn tree_mode_file_indices_cover_all() {
        let files = vec![cf("z.rs"), cf("d/a.rs"), cf("d/b.rs")];
        let folded = std::collections::HashSet::new();
        let mut idx: Vec<usize> = commit_file_rows(&files, true, &folded)
            .into_iter()
            .filter_map(|r| match r.kind {
                CommitRowKind::File(i) => Some(i),
                _ => None,
            })
            .collect();
        idx.sort();
        assert_eq!(idx, vec![0, 1, 2]);
    }
}
