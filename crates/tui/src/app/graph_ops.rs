use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphOp {
    CherryPick,
    Revert,
    Merge,
    RebaseOnto,
    CheckoutDetached,
}

impl GraphOp {
    pub(super) fn question(self, short: &str) -> String {
        match self {
            GraphOp::CherryPick => format!("Cherry-pick {short} onto HEAD? (y/n)"),
            GraphOp::Revert => format!("Revert {short}? (y/n)"),
            GraphOp::Merge => format!("Merge {short} into current branch? (y/n)"),
            GraphOp::RebaseOnto => format!("Rebase current branch onto {short}? (y/n)"),
            GraphOp::CheckoutDetached => format!("Check out {short} (detached HEAD)? (y/n)"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RefTarget {
    Branch(String),
    RemoteBranch(String),
    Tag(String),
}

impl RefTarget {
    pub(super) fn describe(&self) -> String {
        match self {
            RefTarget::Branch(n) => format!("branch {n}"),
            RefTarget::RemoteBranch(n) => format!("remote {n}"),
            RefTarget::Tag(n) => format!("tag {n}"),
        }
    }
}

pub(super) fn short_oid(oid: &Oid) -> String {
    let hex = oid.to_string();
    hex[..7.min(hex.len())].to_string()
}

pub(super) fn numbered(refs: &[RefTarget]) -> String {
    refs.iter()
        .enumerate()
        .map(|(i, r)| format!("{}) {}", i + 1, r.describe()))
        .collect::<Vec<_>>()
        .join("  ")
}

pub fn seq_label(kind: repo::SeqState) -> &'static str {
    match kind {
        repo::SeqState::Rebase => "Rebase",
        repo::SeqState::RebaseInteractive => "Interactive rebase",
        repo::SeqState::CherryPick => "Cherry-pick",
        repo::SeqState::Revert => "Revert",
        repo::SeqState::Merge => "Merge",
        repo::SeqState::None => "",
    }
}

impl TuiApp {
    pub(super) fn run_ref_op(
        &mut self,
        f: impl FnOnce(&Self, &str) -> Result<(), twit_core::git2::Error>,
        input: &str,
    ) {
        let name = input.trim();
        if name.is_empty() {
            return;
        }
        match f(self, name) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("ref op failed: {e}")),
        }
        self.refresh();
    }

    pub(super) fn merge_label(&self, oid: Oid) -> String {
        use twit_core::repo::RefKind;
        if let Some(row) = self.graph.rows.iter().find(|r| r.id == oid) {
            for r in &row.refs {
                match r.kind {
                    RefKind::LocalBranch if !r.is_head => return format!("branch '{}'", r.name),
                    RefKind::RemoteBranch => return format!("remote-tracking branch '{}'", r.name),
                    RefKind::Tag => return format!("tag '{}'", r.name),
                    _ => {}
                }
            }
        }
        format!("commit '{}'", short_oid(&oid))
    }

    pub(super) fn run_reset(&mut self, oid: Oid, mode: repo::ResetMode) {
        match repo::reset(&self.selected, oid, mode) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("reset failed: {e}")),
        }
        self.refresh();
    }

    pub(super) fn run_graph_op(&mut self, op: GraphOp, oid: Oid) {
        match op {
            GraphOp::CherryPick => {
                let r = repo::cherry_pick(&self.selected, oid);
                self.apply_seq_outcome("cherry-pick", r);
            }
            GraphOp::Revert => {
                let r = repo::revert(&self.selected, oid);
                self.apply_seq_outcome("revert", r);
            }
            GraphOp::Merge => {
                let label = self.merge_label(oid);
                let r = repo::merge(&self.selected, oid, &label);
                self.apply_seq_outcome("merge", r);
            }
            GraphOp::RebaseOnto => {
                let r = repo::rebase_onto(&self.selected, oid);
                self.apply_seq_outcome("rebase", r);
            }
            GraphOp::CheckoutDetached => {
                match repo::checkout_commit(&self.selected, oid) {
                    Ok(()) => self.error = None,
                    Err(e) => self.error = Some(format!("checkout failed: {e}")),
                }
                self.refresh();
            }
        }
    }

    pub(super) fn apply_seq_outcome(
        &mut self,
        what: &str,
        r: Result<repo::SeqOutcome, twit_core::git2::Error>,
    ) {
        match r {
            Ok(_) => self.error = None,
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.refresh();
    }

    pub(super) fn run_checkout_ref(&mut self, target: &RefTarget) {
        let res = match target {
            RefTarget::Branch(name) => repo::checkout_branch(&self.selected, name),
            RefTarget::RemoteBranch(name) => repo::checkout_tracking(&self.selected, name),
            RefTarget::Tag(_) => return,
        };
        match res {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("checkout failed: {e}")),
        }
        self.refresh();
    }

    pub(super) fn run_delete_ref(&mut self, target: &RefTarget) {
        let res = match target {
            RefTarget::Branch(name) => repo::delete_branch(&self.selected, name),
            RefTarget::Tag(name) => repo::delete_tag(&self.selected, name),
            RefTarget::RemoteBranch(name) => {
                let Some((remote, branch)) = name.split_once('/') else {
                    self.error = Some(format!("invalid remote branch: {name}"));
                    return;
                };
                let (remote, branch) = (remote.to_string(), branch.to_string());
                self.start_remote(RemoteKind::DeleteRemote, Some(remote), vec![branch]);
                return;
            }
        };
        match res {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("delete failed: {e}")),
        }
        self.refresh();
    }

    pub(super) fn seq_continue(&mut self) {
        let Some((kind, _)) = self.seq else {
            return;
        };
        match kind {
            repo::SeqState::RebaseInteractive => self.seq_git_shell("--continue"),
            repo::SeqState::Rebase => {
                let r = repo::rebase_continue(&self.selected);
                self.apply_seq_outcome("rebase --continue", r);
            }
            repo::SeqState::CherryPick => {
                let r = repo::cherry_pick_continue(&self.selected);
                self.apply_seq_outcome("cherry-pick --continue", r);
            }
            repo::SeqState::Revert => {
                let r = repo::revert_continue(&self.selected);
                self.apply_seq_outcome("revert --continue", r);
            }
            repo::SeqState::Merge => {
                let r = repo::merge_continue(&self.selected);
                self.apply_seq_outcome("merge --continue", r);
            }
            repo::SeqState::None => {}
        }
    }

    pub(super) fn seq_abort(&mut self) {
        let Some((kind, _)) = self.seq else {
            return;
        };
        let res = match kind {
            repo::SeqState::RebaseInteractive => {
                self.seq_git_shell("--abort");
                return;
            }
            repo::SeqState::Rebase => repo::rebase_abort(&self.selected),
            repo::SeqState::CherryPick => repo::cherry_pick_abort(&self.selected),
            repo::SeqState::Revert => repo::revert_abort(&self.selected),
            repo::SeqState::Merge => repo::merge_abort(&self.selected),
            repo::SeqState::None => return,
        };
        if let Err(e) = res {
            self.error = Some(format!("abort failed: {e}"));
        } else {
            self.error = None;
        }
        self.refresh();
    }

    pub(super) fn seq_git_shell(&mut self, arg: &str) {
        self.pending_shell = Some(vec![
            "git".to_string(),
            "-C".to_string(),
            self.selected.to_string_lossy().into_owned(),
            "rebase".to_string(),
            arg.to_string(),
        ]);
    }

    pub(super) fn busy_with_seq(&mut self) -> bool {
        if repo::seq_state(&self.selected) != repo::SeqState::None {
            self.error = Some("finish or abort the in-progress operation first".to_string());
            true
        } else {
            false
        }
    }

    pub(super) fn graph_op_prompt(&mut self, action: Action) {
        let Some((row, oid)) = self.graph_target() else {
            return;
        };
        if self.busy_with_seq() {
            return;
        }
        let confirm_op = |op| (Prompt::ConfirmOp { op, oid }, String::new());
        self.prompt = Some(match action {
            Action::GraphReset => (Prompt::Reset { oid }, String::new()),
            Action::GraphCreateBranch => (Prompt::CreateBranch { at: oid }, String::new()),
            Action::GraphCreateTag => (Prompt::CreateTag { at: oid }, String::new()),
            Action::GraphCherryPick => confirm_op(GraphOp::CherryPick),
            Action::GraphRevert => confirm_op(GraphOp::Revert),
            Action::GraphMerge => confirm_op(GraphOp::Merge),
            Action::GraphRebaseOnto => confirm_op(GraphOp::RebaseOnto),
            Action::GraphCheckout => {
                let refs: Vec<RefTarget> = self
                    .row_refs(row)
                    .into_iter()
                    .filter(|r| !matches!(r, RefTarget::Tag(_)))
                    .collect();
                if refs.is_empty() {
                    confirm_op(GraphOp::CheckoutDetached)
                } else {
                    (Prompt::Checkout { oid, refs }, String::new())
                }
            }
            Action::GraphRenameBranch => {
                use twit_core::repo::RefKind;
                let names: Vec<String> = self.graph.rows[row]
                    .refs
                    .iter()
                    .filter(|r| r.kind == RefKind::LocalBranch)
                    .map(|r| r.name.clone())
                    .collect();
                match names.len() {
                    0 => {
                        self.error = Some("no local branch on this commit".to_string());
                        return;
                    }
                    1 => {
                        let from = names[0].clone();
                        (Prompt::RenameBranch { from: from.clone() }, from)
                    }
                    _ => (Prompt::PickRenameBranch { names }, String::new()),
                }
            }
            Action::GraphDeleteRef => {
                let refs = self.row_refs(row);
                match refs.len() {
                    0 => {
                        self.error = Some("no branch/tag on this commit".to_string());
                        return;
                    }
                    1 => (
                        Prompt::ConfirmDeleteRef {
                            target: refs[0].clone(),
                        },
                        String::new(),
                    ),
                    _ => (Prompt::DeleteRef { refs }, String::new()),
                }
            }
            _ => return,
        });
    }

    pub(super) fn interactive_rebase(&mut self) {
        let Some((_, oid)) = self.graph_target() else {
            return;
        };
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
        self.pending_shell = Some(vec![
            "git".to_string(),
            "-C".to_string(),
            self.selected.to_string_lossy().into_owned(),
            "rebase".to_string(),
            "-i".to_string(),
            base,
        ]);
    }
}
