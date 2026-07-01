use std::path::Path;

use git2::{
    ApplyLocation, ApplyOptions, Diff, DiffOptions, ErrorCode, Index, IndexAddOption, Oid, Rebase,
    Repository, RepositoryState, Signature,
};

pub fn stage(repo_path: &Path, paths: &[String]) -> Result<(), git2::Error> {
    if paths.is_empty() {
        return Ok(());
    }
    let repo = Repository::open(repo_path)?;
    let mut index = repo.index()?;
    index.add_all(
        paths.iter().map(String::as_str),
        IndexAddOption::DEFAULT,
        None,
    )?;
    index.write()
}

pub fn unstage(repo_path: &Path, paths: &[String]) -> Result<(), git2::Error> {
    if paths.is_empty() {
        return Ok(());
    }
    let repo = Repository::open(repo_path)?;
    match repo.head() {
        Ok(head) => {
            let obj = head.peel(git2::ObjectType::Commit)?;
            repo.reset_default(Some(&obj), paths.iter().map(String::as_str))
        }
        Err(_) => {
            let mut index = repo.index()?;
            for p in paths {
                index.remove_path(Path::new(p))?;
            }
            index.write()
        }
    }
}

pub fn stage_hunk(repo_path: &Path, file: &str, hunk_index: usize) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut opts = DiffOptions::new();
    opts.pathspec(file);
    opts.context_lines(0);
    let diff = repo.diff_index_to_workdir(None, Some(&mut opts))?;
    apply_nth_hunk(&repo, &diff, hunk_index)
}

pub fn unstage_hunk(repo_path: &Path, file: &str, hunk_index: usize) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let mut opts = DiffOptions::new();
    opts.pathspec(file);
    opts.context_lines(0);
    opts.reverse(true);
    let diff = repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))?;
    apply_nth_hunk(&repo, &diff, hunk_index)
}

fn apply_nth_hunk(repo: &Repository, diff: &Diff, target: usize) -> Result<(), git2::Error> {
    let mut counter = 0usize;
    let mut opts = ApplyOptions::new();
    opts.hunk_callback(|_hunk| {
        let take = counter == target;
        counter += 1;
        take
    });
    repo.apply(diff, ApplyLocation::Index, Some(&mut opts))
}

pub fn discard(repo_path: &Path, paths: &[String]) -> Result<(), git2::Error> {
    if paths.is_empty() {
        return Ok(());
    }
    let repo = Repository::open(repo_path)?;

    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return discard_unborn(&repo, paths),
    };

    let obj = head.peel(git2::ObjectType::Commit)?;
    repo.reset_default(Some(&obj), paths.iter().map(String::as_str))?;

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.force().remove_untracked(true);
    for p in paths {
        cb.path(p);
    }
    repo.checkout_head(Some(&mut cb))
}

fn discard_unborn(repo: &Repository, paths: &[String]) -> Result<(), git2::Error> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("no workdir (bare repository)"))?
        .to_path_buf();
    let mut index = repo.index()?;
    for p in paths {
        index.remove_all([p].iter(), None)?;
    }
    index.write()?;
    for p in paths {
        let abs = workdir.join(p);
        if abs.is_dir() {
            let _ = std::fs::remove_dir_all(&abs);
        } else if abs.exists() {
            let _ = std::fs::remove_file(&abs);
        }
    }
    Ok(())
}

pub fn checkout_branch(repo_path: &Path, branch: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let refname = format!("refs/heads/{branch}");
    let obj = repo.revparse_single(&refname)?;
    repo.checkout_tree(&obj, Some(git2::build::CheckoutBuilder::new().safe()))?;
    repo.set_head(&refname)
}

pub fn checkout_commit(repo_path: &Path, oid: Oid) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let obj = repo.find_object(oid, None)?;
    repo.checkout_tree(&obj, Some(git2::build::CheckoutBuilder::new().safe()))?;
    repo.set_head_detached(oid)
}

pub enum SeqOutcome {
    Done,
    Conflicts(Vec<String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeqState {
    None,
    Rebase,
    CherryPick,
    Revert,
}

pub fn seq_state(repo_path: &Path) -> SeqState {
    match Repository::open(repo_path).map(|r| r.state()) {
        Ok(RepositoryState::Rebase)
        | Ok(RepositoryState::RebaseInteractive)
        | Ok(RepositoryState::RebaseMerge) => SeqState::Rebase,
        Ok(RepositoryState::CherryPick) | Ok(RepositoryState::CherryPickSequence) => {
            SeqState::CherryPick
        }
        Ok(RepositoryState::Revert) | Ok(RepositoryState::RevertSequence) => SeqState::Revert,
        _ => SeqState::None,
    }
}

pub fn seq_conflicts(repo_path: &Path) -> Vec<String> {
    match Repository::open(repo_path).and_then(|r| r.index()) {
        Ok(index) => conflict_paths(&index),
        Err(_) => Vec::new(),
    }
}

pub fn rebase_onto(repo_path: &Path, onto_oid: Oid) -> Result<SeqOutcome, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let head = repo.head()?;
    let branch = repo.reference_to_annotated_commit(&head)?;
    let onto = repo.find_annotated_commit(onto_oid)?;

    let mut rebase = repo.rebase(Some(&branch), Some(&onto), None, None)?;
    drive_rebase(&repo, &mut rebase)
}

pub fn rebase_continue(repo_path: &Path) -> Result<SeqOutcome, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut rebase = repo.open_rebase(None)?;
    if repo.index()?.has_conflicts() {
        return Ok(SeqOutcome::Conflicts(conflict_paths(&repo.index()?)));
    }

    let sig = repo.signature()?;
    commit_op(&mut rebase, &sig)?;
    drive_rebase(&repo, &mut rebase)
}

pub fn rebase_abort(repo_path: &Path) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut rebase = repo.open_rebase(None)?;
    rebase.abort()
}

pub fn cherry_pick(repo_path: &Path, oid: Oid) -> Result<SeqOutcome, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    let mut opts = git2::CherrypickOptions::new();
    if commit.parent_count() > 1 {
        opts.mainline(1);
    }
    repo.cherrypick(&commit, Some(&mut opts))?;
    finish_cherry_pick(&repo, &commit)
}

pub fn cherry_pick_continue(repo_path: &Path) -> Result<SeqOutcome, git2::Error> {
    let repo = Repository::open(repo_path)?;
    if repo.index()?.has_conflicts() {
        return Ok(SeqOutcome::Conflicts(conflict_paths(&repo.index()?)));
    }
    let pick_oid = pseudo_ref_oid(&repo, "CHERRY_PICK_HEAD")?;
    let commit = repo.find_commit(pick_oid)?;
    finish_cherry_pick(&repo, &commit)
}

pub fn cherry_pick_abort(repo_path: &Path) -> Result<(), git2::Error> {
    reset_hard_to_head(repo_path)
}

fn pseudo_ref_oid(repo: &Repository, name: &str) -> Result<Oid, git2::Error> {
    let raw = std::fs::read_to_string(repo.path().join(name))
        .map_err(|e| git2::Error::from_str(&format!("read {name}: {e}")))?;
    Oid::from_str(raw.trim())
}

pub fn revert(repo_path: &Path, oid: Oid) -> Result<SeqOutcome, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    let mut opts = git2::RevertOptions::new();
    if commit.parent_count() > 1 {
        opts.mainline(1);
    }
    repo.revert(&commit, Some(&mut opts))?;
    finish_revert(&repo, &commit)
}

pub fn revert_continue(repo_path: &Path) -> Result<SeqOutcome, git2::Error> {
    let repo = Repository::open(repo_path)?;
    if repo.index()?.has_conflicts() {
        return Ok(SeqOutcome::Conflicts(conflict_paths(&repo.index()?)));
    }
    let reverted = repo.find_commit(pseudo_ref_oid(&repo, "REVERT_HEAD")?)?;
    finish_revert(&repo, &reverted)
}

pub fn revert_abort(repo_path: &Path) -> Result<(), git2::Error> {
    reset_hard_to_head(repo_path)
}

fn finish_revert(repo: &Repository, reverted: &git2::Commit) -> Result<SeqOutcome, git2::Error> {
    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Ok(SeqOutcome::Conflicts(conflict_paths(&index)));
    }
    let tree = repo.find_tree(index.write_tree()?)?;
    let head = repo.head()?.peel_to_commit()?;
    let sig = repo.signature()?;
    let subject = reverted.summary().ok().flatten().unwrap_or("commit");
    let msg = format!(
        "Revert \"{subject}\"\n\nThis reverts commit {}.\n",
        reverted.id()
    );
    repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &[&head])?;
    repo.cleanup_state()?;
    Ok(SeqOutcome::Done)
}

fn reset_hard_to_head(repo_path: &Path) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let head = repo.head()?.peel_to_commit()?;
    repo.reset(head.as_object(), git2::ResetType::Hard, None)?;
    repo.cleanup_state()
}

fn finish_cherry_pick(repo: &Repository, picked: &git2::Commit) -> Result<SeqOutcome, git2::Error> {
    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Ok(SeqOutcome::Conflicts(conflict_paths(&index)));
    }
    let tree = repo.find_tree(index.write_tree()?)?;
    let head = repo.head()?.peel_to_commit()?;
    let committer = repo.signature()?;

    repo.commit(
        Some("HEAD"),
        &picked.author(),
        &committer,
        picked.message().unwrap_or(""),
        &tree,
        &[&head],
    )?;
    repo.cleanup_state()?;
    Ok(SeqOutcome::Done)
}

pub fn create_branch(repo_path: &Path, name: &str, oid: Oid) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let commit = repo.find_commit(oid)?;
    repo.branch(name, &commit, false)?;
    Ok(())
}

pub fn rename_branch(repo_path: &Path, from: &str, to: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut branch = repo.find_branch(from, git2::BranchType::Local)?;
    branch.rename(to, false)?;
    Ok(())
}

pub fn delete_branch(repo_path: &Path, name: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut branch = repo.find_branch(name, git2::BranchType::Local)?;
    branch.delete()
}

pub fn create_tag(repo_path: &Path, name: &str, oid: Oid) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let obj = repo.find_object(oid, None)?;
    repo.tag_lightweight(name, &obj, false)?;
    Ok(())
}

pub fn delete_tag(repo_path: &Path, name: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    repo.tag_delete(name)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

impl ResetMode {
    pub fn label(self) -> &'static str {
        match self {
            ResetMode::Soft => "Soft",
            ResetMode::Mixed => "Mixed",
            ResetMode::Hard => "Hard",
        }
    }
}

pub fn reset(repo_path: &Path, oid: Oid, mode: ResetMode) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let obj = repo.find_object(oid, None)?;
    let kind = match mode {
        ResetMode::Soft => git2::ResetType::Soft,
        ResetMode::Mixed => git2::ResetType::Mixed,
        ResetMode::Hard => git2::ResetType::Hard,
    };
    repo.reset(&obj, kind, None)
}

pub struct StashEntry {
    pub index: usize,
    pub message: String,
}

pub fn stash_save(repo_path: &Path, message: Option<&str>) -> Result<(), git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    let sig = repo.signature()?;
    repo.stash_save2(&sig, message, Some(git2::StashFlags::INCLUDE_UNTRACKED))?;
    Ok(())
}

pub fn stash_list(repo_path: &Path) -> Vec<StashEntry> {
    let mut out = Vec::new();
    if let Ok(mut repo) = Repository::open(repo_path) {
        let _ = repo.stash_foreach(|index, message, _oid| {
            out.push(StashEntry {
                index,
                message: message.to_string(),
            });
            true
        });
    }
    out
}

pub fn stash_pop(repo_path: &Path, index: usize) -> Result<(), git2::Error> {
    Repository::open(repo_path)?.stash_pop(index, None)
}

pub fn stash_apply(repo_path: &Path, index: usize) -> Result<(), git2::Error> {
    Repository::open(repo_path)?.stash_apply(index, None)
}

pub fn stash_drop(repo_path: &Path, index: usize) -> Result<(), git2::Error> {
    Repository::open(repo_path)?.stash_drop(index)
}

fn drive_rebase(repo: &Repository, rebase: &mut Rebase) -> Result<SeqOutcome, git2::Error> {
    let sig = repo.signature()?;
    while let Some(op) = rebase.next() {
        op?;
        let index = repo.index()?;
        if index.has_conflicts() {
            return Ok(SeqOutcome::Conflicts(conflict_paths(&index)));
        }
        commit_op(rebase, &sig)?;
    }
    rebase.finish(Some(&sig))?;
    Ok(SeqOutcome::Done)
}

fn commit_op(rebase: &mut Rebase, sig: &Signature) -> Result<(), git2::Error> {
    match rebase.commit(None, sig, None) {
        Ok(_) => Ok(()),
        Err(e) if e.code() == ErrorCode::Applied => Ok(()),
        Err(e) => Err(e),
    }
}

fn conflict_paths(index: &Index) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(conflicts) = index.conflicts() {
        for c in conflicts.flatten() {
            if let Some(entry) = c.our.or(c.their).or(c.ancestor)
                && let Ok(p) = std::str::from_utf8(&entry.path) {
                    let p = p.to_string();
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
        }
    }
    out
}

pub fn commit(repo_path: &Path, message: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let sig = repo.signature()?;

    let mut index = repo.index()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    let parents = match repo.head() {
        Ok(head) => vec![head.peel_to_commit()?],
        Err(_) => Vec::new(),
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;
    Ok(())
}
