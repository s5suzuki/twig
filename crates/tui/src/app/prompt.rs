use super::*;

pub(super) fn pick<T: Clone>(items: &[T], c: char) -> Option<T> {
    let idx = c.to_digit(10)? as usize;
    (1..=items.len())
        .contains(&idx)
        .then(|| items[idx - 1].clone())
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Prompt {
    Commit,
    Amend,
    ConfirmAmendPushed,
    ConfirmDiscardFiles {
        paths: Vec<String>,
        label: String,
    },
    ConfirmDiscardLines {
        path: String,
        lo: usize,
        hi: usize,
    },
    CreateBranch {
        at: Oid,
    },
    RenameBranch {
        from: String,
    },
    CreateTag {
        at: Oid,
    },
    Reset {
        oid: Oid,
    },
    ConfirmResetHard {
        oid: Oid,
    },
    ConfirmOp {
        op: GraphOp,
        oid: Oid,
    },
    Checkout {
        oid: Oid,
        refs: Vec<RefTarget>,
    },
    DeleteRef {
        refs: Vec<RefTarget>,
    },
    ConfirmDeleteRef {
        target: RefTarget,
    },
    PickRenameBranch {
        names: Vec<String>,
    },
    ConfirmForcePush {
        remote: String,
        refspec: String,
    },
    ConfirmSeqAbort,
    StashOp {
        index: usize,
    },
    ConfirmStashDrop {
        index: usize,
    },
    DiffFind,
    EditGraphLimit,
    SearchQuery,
    SearchInclude,
    SearchExclude,
    SearchReplace,
    ConfirmSearchReplace {
        replacement: String,
    },
    ConfirmSubmodule {
        kind: RemoteKind,
        parent: PathBuf,
        name: String,
    },
}

impl Prompt {
    pub fn wants_text(&self) -> bool {
        matches!(
            self,
            Prompt::Commit
                | Prompt::Amend
                | Prompt::CreateBranch { .. }
                | Prompt::RenameBranch { .. }
                | Prompt::CreateTag { .. }
                | Prompt::DiffFind
                | Prompt::EditGraphLimit
                | Prompt::SearchQuery
                | Prompt::SearchInclude
                | Prompt::SearchExclude
                | Prompt::SearchReplace
        )
    }

    pub fn wants_popup(&self) -> bool {
        matches!(self, Prompt::Commit | Prompt::Amend) || self.is_confirm() || self.is_choice()
    }

    fn is_confirm(&self) -> bool {
        matches!(
            self,
            Prompt::ConfirmAmendPushed
                | Prompt::ConfirmDiscardFiles { .. }
                | Prompt::ConfirmDiscardLines { .. }
                | Prompt::ConfirmResetHard { .. }
                | Prompt::ConfirmOp { .. }
                | Prompt::ConfirmDeleteRef { .. }
                | Prompt::ConfirmForcePush { .. }
                | Prompt::ConfirmSeqAbort
                | Prompt::ConfirmStashDrop { .. }
                | Prompt::ConfirmSearchReplace { .. }
                | Prompt::ConfirmSubmodule { .. }
        )
    }

    fn is_choice(&self) -> bool {
        matches!(
            self,
            Prompt::Reset { .. }
                | Prompt::Checkout { .. }
                | Prompt::DeleteRef { .. }
                | Prompt::PickRenameBranch { .. }
                | Prompt::StashOp { .. }
        )
    }

    pub fn hint(&self) -> &'static str {
        if self.wants_text() {
            "Enter: confirm   Esc: cancel"
        } else if self.is_choice() {
            "press the highlighted key   Esc: cancel"
        } else {
            "y: confirm   n / Esc: cancel"
        }
    }

    pub fn label(&self) -> String {
        match self {
            Prompt::Commit => "Commit message:".to_string(),
            Prompt::Amend => "Amend message:".to_string(),
            Prompt::ConfirmAmendPushed => "HEAD is already pushed. Amend anyway? (y/n)".to_string(),
            Prompt::ConfirmDiscardFiles { label, .. } => {
                format!("Discard changes to {label}? (y/n)")
            }
            Prompt::ConfirmDiscardLines { path, .. } => {
                format!("Discard selected lines in {path}? (y/n)")
            }
            Prompt::CreateBranch { at } => format!("Branch name at {}:", short_oid(at)),
            Prompt::RenameBranch { from } => format!("Rename branch {from} to:"),
            Prompt::CreateTag { at } => format!("Tag name at {}:", short_oid(at)),
            Prompt::Reset { oid } => format!(
                "Reset HEAD to {}: (s)oft / (m)ixed / (h)ard",
                short_oid(oid)
            ),
            Prompt::ConfirmResetHard { .. } => {
                "Hard reset discards working tree changes. Continue? (y/n)".to_string()
            }
            Prompt::ConfirmOp { op, oid } => op.question(&short_oid(oid)),
            Prompt::Checkout { refs, .. } => {
                format!("Checkout: {}  c) commit (detached)", numbered(refs))
            }
            Prompt::DeleteRef { refs } => format!("Delete: {}", numbered(refs)),
            Prompt::ConfirmDeleteRef { target } => {
                format!("Delete {}? (y/n)", target.describe())
            }
            Prompt::PickRenameBranch { names } => format!(
                "Rename: {}",
                names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| format!("{}) {n}", i + 1))
                    .collect::<Vec<_>>()
                    .join("  ")
            ),
            Prompt::ConfirmForcePush { remote, .. } => {
                format!("Force push current branch to {remote}? (y/n)")
            }
            Prompt::ConfirmSeqAbort => "Abort the in-progress operation? (y/n)".to_string(),
            Prompt::StashOp { index } => {
                format!("stash@{{{index}}}: (p)op / (a)pply / (d)rop")
            }
            Prompt::ConfirmStashDrop { index } => {
                format!("Drop stash@{{{index}}}? (y/n)")
            }
            Prompt::DiffFind => "Find in diff:".to_string(),
            Prompt::EditGraphLimit => "Graph commit limit:".to_string(),
            Prompt::SearchQuery => "Search repository:".to_string(),
            Prompt::SearchInclude => "Files to include (glob):".to_string(),
            Prompt::SearchExclude => "Files to exclude (glob):".to_string(),
            Prompt::SearchReplace => "Replace matches with:".to_string(),
            Prompt::ConfirmSearchReplace { replacement } => {
                format!("Replace all matches with \"{replacement}\"? (y/n)")
            }
            Prompt::ConfirmSubmodule { kind, name, .. } => match kind {
                RemoteKind::SubmoduleInit => {
                    format!("Initialize submodule {name} (clone)? (y/n)")
                }
                _ => format!("Update submodule {name} to the recorded commit? (y/n)"),
            },
        }
    }
}

impl TuiApp {
    pub(super) fn handle_prompt_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        let Some((kind, input)) = self.prompt.as_mut() else {
            return;
        };
        if ev.modifiers.contains(KeyModifiers::CONTROL) {
            if ev.code == KeyCode::Char('c') {
                self.prompt = None;
            }
            return;
        }
        if kind.wants_text() {
            match ev.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Enter => self.submit_prompt(),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => input.push(c),
                _ => {}
            }
        } else if kind.is_choice() {
            match ev.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Char(c) => self.handle_choice(c),
                _ => {}
            }
        } else {
            match ev.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.submit_prompt(),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.prompt = None,
                _ => {}
            }
        }
    }

    pub(super) fn handle_choice(&mut self, c: char) {
        let Some((kind, _)) = self.prompt.clone() else {
            return;
        };
        match kind {
            Prompt::Reset { oid } => match c {
                's' => self.take_prompt_and(|app| app.run_reset(oid, repo::ResetMode::Soft)),
                'm' => self.take_prompt_and(|app| app.run_reset(oid, repo::ResetMode::Mixed)),
                'h' => self.prompt = Some((Prompt::ConfirmResetHard { oid }, String::new())),
                _ => {}
            },
            Prompt::Checkout { oid, refs } => {
                if c == 'c' {
                    self.prompt = Some((
                        Prompt::ConfirmOp {
                            op: GraphOp::CheckoutDetached,
                            oid,
                        },
                        String::new(),
                    ));
                } else if let Some(target) = pick(&refs, c) {
                    self.take_prompt_and(|app| app.run_checkout_ref(&target));
                }
            }
            Prompt::DeleteRef { refs } => {
                if let Some(target) = pick(&refs, c) {
                    self.prompt = Some((Prompt::ConfirmDeleteRef { target }, String::new()));
                }
            }
            Prompt::PickRenameBranch { names } => {
                if let Some(from) = pick(&names, c) {
                    self.prompt = Some((Prompt::RenameBranch { from: from.clone() }, from));
                }
            }
            Prompt::StashOp { index } => match c {
                'p' => self.take_prompt_and(|app| {
                    app.run_stash_op(repo::stash_pop(&app.selected, index), "stash pop")
                }),
                'a' => self.take_prompt_and(|app| {
                    app.run_stash_op(repo::stash_apply(&app.selected, index), "stash apply")
                }),
                'd' => self.prompt = Some((Prompt::ConfirmStashDrop { index }, String::new())),
                _ => {}
            },
            _ => {}
        }
    }

    pub(super) fn take_prompt_and(&mut self, f: impl FnOnce(&mut Self)) {
        self.prompt = None;
        f(self);
    }

    pub(super) fn submit_prompt(&mut self) {
        let Some((kind, input)) = self.prompt.take() else {
            return;
        };
        match kind {
            Prompt::Commit => self.run_commit(&input),
            Prompt::Amend => {
                if input.trim().is_empty() {
                    return;
                }
                if repo::head_is_pushed(&self.selected) {
                    self.prompt = Some((Prompt::ConfirmAmendPushed, input));
                } else {
                    self.run_amend(&input);
                }
            }
            Prompt::ConfirmAmendPushed => self.run_amend(&input),
            Prompt::ConfirmDiscardFiles { paths, .. } => self.run_discard_files(&paths),
            Prompt::ConfirmDiscardLines { path, lo, hi } => self.run_discard_lines(&path, lo, hi),
            Prompt::CreateBranch { at } => {
                self.run_ref_op(|s, name| repo::create_branch(&s.selected, name, at), &input)
            }
            Prompt::RenameBranch { from } => self.run_ref_op(
                |s, name| repo::rename_branch(&s.selected, &from, name),
                &input,
            ),
            Prompt::CreateTag { at } => {
                self.run_ref_op(|s, name| repo::create_tag(&s.selected, name, at), &input)
            }
            Prompt::ConfirmResetHard { oid } => self.run_reset(oid, repo::ResetMode::Hard),
            Prompt::ConfirmOp { op, oid } => self.run_graph_op(op, oid),
            Prompt::ConfirmDeleteRef { target } => self.run_delete_ref(&target),
            Prompt::ConfirmForcePush { remote, refspec } => {
                self.start_remote(RemoteKind::ForcePush, Some(remote), vec![refspec]);
            }
            Prompt::ConfirmSeqAbort => self.seq_abort(),
            Prompt::ConfirmStashDrop { index } => {
                self.run_stash_op(repo::stash_drop(&self.selected, index), "stash drop");
            }
            Prompt::DiffFind => {
                let query = input.trim().to_string();
                if query.is_empty() {
                    self.diff_find = None;
                } else {
                    let on_match = self
                        .diff
                        .rows
                        .get(self.diff_nav.cursor)
                        .is_some_and(|r| row_contains(r, &query));
                    self.diff_find = Some(query);
                    if !on_match {
                        self.jump_find(true);
                    }
                }
            }
            Prompt::EditGraphLimit => match input.trim().parse::<usize>() {
                Ok(n) if n > 0 => {
                    self.config.graph_commit_limit = n;
                    self.config.save();
                    self.refresh();
                }
                _ => self.error = Some(format!("invalid commit limit: {}", input.trim())),
            },
            Prompt::SearchQuery => self.run_search(input.trim()),
            Prompt::SearchInclude => {
                self.search.include = input.trim().to_string();
                let query = self.search.query.clone();
                if !query.is_empty() {
                    self.run_search(&query);
                }
            }
            Prompt::SearchExclude => {
                self.search.exclude = input.trim().to_string();
                let query = self.search.query.clone();
                if !query.is_empty() {
                    self.run_search(&query);
                }
            }
            Prompt::SearchReplace => {
                self.prompt = Some((
                    Prompt::ConfirmSearchReplace { replacement: input },
                    String::new(),
                ));
            }
            Prompt::ConfirmSearchReplace { replacement } => {
                self.run_search_replace(&replacement);
            }
            Prompt::ConfirmSubmodule { kind, parent, name } => {
                self.start_remote_submodule(kind, parent, name);
            }
            Prompt::Reset { .. }
            | Prompt::Checkout { .. }
            | Prompt::DeleteRef { .. }
            | Prompt::PickRenameBranch { .. }
            | Prompt::StashOp { .. } => {}
        }
    }
}
