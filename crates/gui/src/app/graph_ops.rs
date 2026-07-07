use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeqKind {
    Rebase,
    RebaseInteractive,
    CherryPick,
    Revert,
    Merge,
}

impl SeqKind {
    pub fn label(self) -> &'static str {
        match self {
            SeqKind::Rebase => "Rebase",
            SeqKind::RebaseInteractive => "Interactive rebase",
            SeqKind::CherryPick => "Cherry-pick",
            SeqKind::Revert => "Revert",
            SeqKind::Merge => "Merge",
        }
    }
}

pub struct SeqStatus {
    pub kind: SeqKind,
    pub conflicts: Vec<String>,
}

pub enum RefPrompt {
    CreateBranch { at: Oid },
    RenameBranch { from: String },
    CreateTag { at: Oid },
}

pub enum DeleteTarget {
    Branch(String),
    Tag(String),
    RemoteBranch(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GraphOp {
    CherryPick,
    Revert,
    RebaseOnto,
    Checkout,
}

impl GraphOp {
    pub fn title(self) -> &'static str {
        match self {
            GraphOp::CherryPick => "Cherry-pick commit",
            GraphOp::Revert => "Revert commit",
            GraphOp::RebaseOnto => "Rebase onto commit",
            GraphOp::Checkout => "Check out commit",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            GraphOp::CherryPick => {
                "Apply this commit's changes onto the current branch as a new commit."
            }
            GraphOp::Revert => {
                "Create a new commit on the current branch that undoes this commit's changes."
            }
            GraphOp::RebaseOnto => {
                "Replay the current branch onto this commit. This rewrites the branch's commits."
            }
            GraphOp::Checkout => "Check out this commit directly (detached HEAD).",
        }
    }
}

impl App {
    pub fn interactive_rebase(&mut self, oid: Oid) {
        if self.busy_with_seq() {
            return;
        }
        let base = match repo::commit_parent_count(&self.selected, oid) {
            Ok(0) => "--root".to_string(),
            Ok(_) => format!("{oid}^"),
            Err(e) => {
                self.error = Some(format!("rebase failed: {e}"));
                return;
            }
        };
        let dir = sh_quote(&self.selected.to_string_lossy());
        self.pending_shell_cmd = Some(format!("git -C {dir} rebase -i {base}"));
        self.shell_open = true;
        self.focus = Pane::Terminal;
    }

    pub(super) fn busy_with_seq(&mut self) -> bool {
        if self.seq.is_some() {
            self.error = Some("Finish or abort the in-progress operation first".to_string());
            true
        } else {
            false
        }
    }

    pub fn switch_branch(&mut self, name: String) {
        if self.busy_with_seq() {
            return;
        }
        match repo::checkout_branch(&self.selected, &name) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("switch failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn checkout_commit(&mut self, oid: Oid) {
        if self.busy_with_seq() {
            return;
        }
        match repo::checkout_commit(&self.selected, oid) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("checkout failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn rebase_onto(&mut self, onto: Oid) {
        let r = repo::rebase_onto(&self.selected, onto);
        self.apply_seq_outcome(SeqKind::Rebase, r, "rebase");
    }

    pub fn cherry_pick(&mut self, oid: Oid) {
        let r = repo::cherry_pick(&self.selected, oid);
        self.apply_seq_outcome(SeqKind::CherryPick, r, "cherry-pick");
    }

    pub fn revert(&mut self, oid: Oid) {
        let r = repo::revert(&self.selected, oid);
        self.apply_seq_outcome(SeqKind::Revert, r, "revert");
    }

    pub fn seq_continue(&mut self) {
        let Some(kind) = self.seq.as_ref().map(|s| s.kind) else {
            return;
        };
        let r = match kind {
            SeqKind::Rebase => repo::rebase_continue(&self.selected),
            SeqKind::RebaseInteractive => return,
            SeqKind::CherryPick => repo::cherry_pick_continue(&self.selected),
            SeqKind::Revert => repo::revert_continue(&self.selected),
            SeqKind::Merge => repo::merge_continue(&self.selected),
        };
        self.apply_seq_outcome(kind, r, "continue");
    }

    pub fn seq_abort(&mut self) {
        let Some(kind) = self.seq.as_ref().map(|s| s.kind) else {
            return;
        };
        let r = match kind {
            SeqKind::Rebase => repo::rebase_abort(&self.selected),
            SeqKind::RebaseInteractive => return,
            SeqKind::CherryPick => repo::cherry_pick_abort(&self.selected),
            SeqKind::Revert => repo::revert_abort(&self.selected),
            SeqKind::Merge => repo::merge_abort(&self.selected),
        };
        if let Err(e) = r {
            self.error = Some(format!("{} --abort failed: {e}", kind.label()));
        }
        self.seq = None;
        self.after_commit_topology_change();
    }

    pub(super) fn apply_seq_outcome(
        &mut self,
        kind: SeqKind,
        r: Result<repo::SeqOutcome, git2::Error>,
        what: &str,
    ) {
        match r {
            Ok(repo::SeqOutcome::Done) => {
                self.seq = None;
                self.error = None;
            }
            Ok(repo::SeqOutcome::Conflicts(files)) => {
                self.seq = Some(SeqStatus {
                    kind,
                    conflicts: files,
                });
            }
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn begin_create_branch(&mut self, at: Oid) {
        if self.busy_with_seq() {
            return;
        }
        self.name_input = String::new();
        self.name_input_focus = true;
        self.ref_prompt = Some(RefPrompt::CreateBranch { at });
    }

    pub fn begin_rename_branch(&mut self, from: String) {
        self.name_input = from.clone();
        self.name_input_focus = true;
        self.ref_prompt = Some(RefPrompt::RenameBranch { from });
    }

    pub fn begin_create_tag(&mut self, at: Oid) {
        self.name_input = String::new();
        self.name_input_focus = true;
        self.ref_prompt = Some(RefPrompt::CreateTag { at });
    }

    pub fn commit_ref_prompt(&mut self, switch: bool) {
        let name = self.name_input.trim().to_string();
        let Some(prompt) = self.ref_prompt.take() else {
            return;
        };
        if name.is_empty() {
            self.error = Some("Name is empty".to_string());
            return;
        }
        let res = match &prompt {
            RefPrompt::CreateBranch { at } => repo::create_branch(&self.selected, &name, *at)
                .and_then(|()| {
                    if switch {
                        repo::checkout_branch(&self.selected, &name)
                    } else {
                        Ok(())
                    }
                }),
            RefPrompt::RenameBranch { from } => repo::rename_branch(&self.selected, from, &name),
            RefPrompt::CreateTag { at } => repo::create_tag(&self.selected, &name, *at),
        };
        match res {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("ref op failed: {e}")),
        }

        self.after_commit_topology_change();
    }

    pub fn delete_ref(&mut self, target: &DeleteTarget) {
        let res = match target {
            DeleteTarget::Branch(name) => repo::delete_branch(&self.selected, name),
            DeleteTarget::Tag(name) => repo::delete_tag(&self.selected, name),
            DeleteTarget::RemoteBranch(_) => return,
        };
        if let Err(e) = res {
            self.error = Some(format!("delete failed: {e}"));
        }
        self.reload();
    }

    pub fn run_confirmed_op(&mut self) {
        let Some((op, oid)) = self.confirm_op.take() else {
            return;
        };
        match op {
            GraphOp::CherryPick => self.cherry_pick(oid),
            GraphOp::Revert => self.revert(oid),
            GraphOp::RebaseOnto => self.rebase_onto(oid),
            GraphOp::Checkout => self.checkout_commit(oid),
        }
    }

    pub fn do_reset(&mut self, oid: Oid, mode: repo::ResetMode) {
        if self.busy_with_seq() {
            return;
        }
        match repo::reset(&self.selected, oid, mode) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("reset failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn checkout_tracking(&mut self, remote_ref: String) {
        if self.busy_with_seq() {
            return;
        }
        match repo::checkout_tracking(&self.selected, &remote_ref) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("checkout failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub(super) fn after_commit_topology_change(&mut self) {
        self.selected_file = None;
        self.clear_commit_selection();
        self.diff = empty_diff();
        self.reload();
    }

    pub(super) fn sync_seq_state(&mut self) {
        let kind = match repo::seq_state(&self.selected) {
            repo::SeqState::None => {
                self.seq = None;
                return;
            }
            repo::SeqState::Rebase => SeqKind::Rebase,
            repo::SeqState::RebaseInteractive => SeqKind::RebaseInteractive,
            repo::SeqState::CherryPick => SeqKind::CherryPick,
            repo::SeqState::Revert => SeqKind::Revert,
            repo::SeqState::Merge => SeqKind::Merge,
        };
        let conflicts = repo::seq_conflicts(&self.selected);
        match &mut self.seq {
            Some(s) => {
                s.kind = kind;
                s.conflicts = conflicts;
            }
            None => self.seq = Some(SeqStatus { kind, conflicts }),
        }
    }
}
