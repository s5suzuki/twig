use super::*;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChangesItem {
    Group {
        staged: bool,
    },
    Folder {
        name: String,
        path: String,
        staged: bool,
        open: bool,
        depth: usize,
    },
    File {
        path: String,
        staged: bool,
        depth: usize,
    },
    StashHeader,
    Stash(usize),
}

impl ChangesItem {
    pub fn depth(&self) -> usize {
        match self {
            ChangesItem::Group { .. } | ChangesItem::StashHeader => 0,
            ChangesItem::Folder { depth, .. } | ChangesItem::File { depth, .. } => *depth,
            ChangesItem::Stash(_) => 1,
        }
    }
}

impl TuiApp {
    pub fn changes_items(&self) -> Vec<ChangesItem> {
        let mut out = Vec::new();
        out.push(ChangesItem::Group { staged: true });
        self.push_side_items(true, &mut out);
        out.push(ChangesItem::Group { staged: false });
        self.push_side_items(false, &mut out);
        if !self.stashes.is_empty() {
            out.push(ChangesItem::StashHeader);
            out.extend(self.stashes.iter().map(|s| ChangesItem::Stash(s.index)));
        }
        out
    }

    pub(super) fn push_side_items(&self, staged: bool, out: &mut Vec<ChangesItem>) {
        let entries = if staged { &self.staged } else { &self.unstaged };
        let files: Vec<CommitFile> = entries
            .iter()
            .map(|e| CommitFile {
                path: e.path.clone(),
                kind: e.kind,
            })
            .collect();
        let folded: HashSet<String> = self
            .changes_folds
            .iter()
            .filter(|(s, _)| *s == staged)
            .map(|(_, p)| p.clone())
            .collect();
        for row in repo::commit_file_rows(&files, true, &folded) {
            out.push(match row.kind {
                repo::CommitRowKind::Folder { name, path, open } => ChangesItem::Folder {
                    name,
                    path,
                    staged,
                    open,
                    depth: row.depth + 1,
                },
                repo::CommitRowKind::File(i) => ChangesItem::File {
                    path: files[i].path.clone(),
                    staged,
                    depth: row.depth + 1,
                },
            });
        }
    }

    pub(super) fn clamp_changes_cursor(&mut self) {
        let n = self.changes_items().len();
        self.changes_cursor = self.changes_cursor.min(n.saturating_sub(1));
    }

    pub(super) fn run_commit(&mut self, msg: &str) {
        let msg = msg.trim();
        if msg.is_empty() {
            self.prompt = Some((Prompt::Commit, String::new()));
            return;
        }
        if self.staged.is_empty() {
            self.error = Some("nothing staged".to_string());
            return;
        }
        match repo::commit(&self.selected, msg) {
            Ok(()) => {
                self.auto_stage_pointer();
                self.refresh();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("commit failed: {e}")),
        }
    }

    pub(super) fn open_amend_prompt(&mut self) {
        if repo::seq_state(&self.selected) != repo::SeqState::None {
            self.error = Some("finish or abort the in-progress operation first".to_string());
            return;
        }
        if !repo::head_has_commit(&self.selected) {
            self.error = Some("nothing to amend".to_string());
            return;
        }
        let msg = repo::head_message(&self.selected)
            .map(|m| m.trim_end().to_string())
            .unwrap_or_default();
        self.prompt = Some((Prompt::Amend, msg));
    }

    pub(super) fn run_amend(&mut self, msg: &str) {
        match repo::amend(&self.selected, Some(msg.trim())) {
            Ok(_) => {
                self.auto_stage_pointer();
                self.refresh();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("amend failed: {e}")),
        }
    }

    pub(super) fn run_discard_files(&mut self, paths: &[String]) {
        match repo::discard(&self.selected, paths) {
            Ok(()) => {
                self.refresh();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("discard failed: {e}")),
        }
    }

    pub(super) fn run_discard_lines(&mut self, path: &str, lo: usize, hi: usize) {
        if let Err(e) = repo::discard_partial(&self.selected, path, &self.diff.rows, lo, hi) {
            self.error = Some(format!("discard failed: {e}"));
        } else {
            self.error = None;
        }
        self.diff_nav.anchor = None;
        self.refresh();
    }

    pub(super) fn auto_stage_pointer(&mut self) {
        if let Some((parent_path, name)) = repo::find_submodule_parent(&self.root, &self.selected)
            && let Err(e) = repo::stage_submodule_pointer(&parent_path, &name)
        {
            self.error = Some(format!("stage submodule pointer failed: {e}"));
        }
    }

    pub(super) fn changes_keys(&mut self, queue: &mut KeyQueue) {
        let half = (self.changes_view_rows / 2).max(1);
        let actions =
            self.keymap
                .resolve(queue, Context::Changes, &mut self.pending_prefix, |_| true);
        for a in actions {
            let items = self.changes_items();
            let last = items.len().saturating_sub(1);
            let cursor = self.changes_cursor.min(last);
            let item = items.get(cursor).cloned();
            match a {
                Action::ChangesDown => self.changes_cursor = (cursor + 1).min(last),
                Action::ChangesUp => self.changes_cursor = cursor.saturating_sub(1),
                Action::ChangesTop => self.changes_cursor = 0,
                Action::ChangesBottom => self.changes_cursor = last,
                Action::ChangesHalfPageDown => self.changes_cursor = (cursor + half).min(last),
                Action::ChangesHalfPageUp => self.changes_cursor = cursor.saturating_sub(half),
                Action::ChangesActivate => match item {
                    Some(ChangesItem::Folder {
                        path, staged, open, ..
                    }) => {
                        self.set_fold(staged, path, open);
                    }
                    item => self.changes_open(item),
                },
                Action::ChangesExpand => match item {
                    Some(ChangesItem::Folder {
                        path,
                        staged,
                        open: false,
                        ..
                    }) => self.set_fold(staged, path, false),
                    Some(ChangesItem::Folder { .. })
                    | Some(ChangesItem::Group { .. })
                    | Some(ChangesItem::StashHeader) => {
                        self.changes_cursor = (cursor + 1).min(last)
                    }
                    item => self.changes_open(item),
                },
                Action::ChangesCollapse => match item {
                    Some(ChangesItem::Folder {
                        path,
                        staged,
                        open: true,
                        ..
                    }) => self.set_fold(staged, path, true),
                    Some(it) => {
                        let depth = it.depth();
                        if depth > 0
                            && let Some(p) = (0..cursor).rev().find(|&i| {
                                items[i].depth() < depth
                                    && matches!(
                                        items[i],
                                        ChangesItem::Folder { .. }
                                            | ChangesItem::Group { .. }
                                            | ChangesItem::StashHeader
                                    )
                            })
                        {
                            self.changes_cursor = p;
                        }
                    }
                    None => {}
                },
                Action::ChangesStageToggle => match item {
                    Some(ChangesItem::File { path, staged, .. }) => {
                        let paths = self.file_paths(staged, &path);
                        self.stage_paths(staged, paths);
                    }
                    Some(ChangesItem::Folder { path, staged, .. }) => {
                        let paths = self.side_stage_paths(staged, Some(&path));
                        self.stage_paths(staged, paths);
                    }
                    Some(ChangesItem::Group { staged }) => {
                        let paths = self.side_stage_paths(staged, None);
                        self.stage_paths(staged, paths);
                    }
                    Some(ChangesItem::Stash(index)) => {
                        self.prompt = Some((Prompt::StashOp { index }, String::new()));
                    }
                    _ => {}
                },
                Action::ChangesEdit => {
                    if let Some(ChangesItem::File { path, .. }) = item {
                        self.pending_editor = Some((self.selected.join(path), None));
                    }
                }
                Action::ChangesDiscard => self.changes_discard(item),
                _ => {}
            }
        }
    }

    pub(super) fn changes_open(&mut self, item: Option<ChangesItem>) {
        match item {
            Some(ChangesItem::File { path, staged, .. }) => {
                self.open_file_diff(path, staged);
                self.pending_focus_jump = self.error.is_none();
            }
            Some(ChangesItem::Stash(index)) => {
                if let Some(oid) = self
                    .stashes
                    .iter()
                    .find(|s| s.index == index)
                    .map(|s| s.oid)
                {
                    self.open_commit_diff(oid);
                    self.pending_focus_jump = self.error.is_none();
                }
            }
            _ => {}
        }
    }

    pub(super) fn set_fold(&mut self, staged: bool, path: String, folded: bool) {
        if folded {
            self.changes_folds.insert((staged, path));
        } else {
            self.changes_folds.remove(&(staged, path));
        }
        self.clamp_changes_cursor();
    }

    pub(super) fn side_paths(&self, staged: bool, folder: Option<&str>) -> Vec<String> {
        let entries = if staged { &self.staged } else { &self.unstaged };
        let prefix = folder.map(|p| format!("{p}/"));
        entries
            .iter()
            .filter(|e| prefix.as_deref().is_none_or(|p| e.path.starts_with(p)))
            .map(|e| e.path.clone())
            .collect()
    }

    pub(super) fn file_paths(&self, staged: bool, path: &str) -> Vec<String> {
        let entries = if staged { &self.staged } else { &self.unstaged };
        match entries
            .iter()
            .find(|e| e.path == path)
            .and_then(|e| e.old_path.as_deref())
        {
            Some(old) if old != path => vec![old.to_string(), path.to_string()],
            _ => vec![path.to_string()],
        }
    }

    pub(super) fn side_stage_paths(&self, staged: bool, folder: Option<&str>) -> Vec<String> {
        self.side_paths(staged, folder)
            .iter()
            .flat_map(|p| self.file_paths(staged, p))
            .collect()
    }

    pub(super) fn changes_discard(&mut self, item: Option<ChangesItem>) {
        let folder = match item {
            Some(ChangesItem::File {
                path,
                staged: false,
                ..
            }) => {
                let paths = self.file_paths(false, &path);
                if !self.config.confirm_discard {
                    self.run_discard_files(&paths);
                    return;
                }
                self.prompt = Some((
                    Prompt::ConfirmDiscardFiles { paths, label: path },
                    String::new(),
                ));
                return;
            }
            Some(ChangesItem::Folder {
                path,
                staged: false,
                ..
            }) => Some(path.clone()),
            Some(ChangesItem::Group { staged: false }) => None,
            _ => return,
        };
        let files = self.side_paths(false, folder.as_deref());
        if files.is_empty() {
            return;
        }
        let label = match &folder {
            Some(path) => format!("{path}/ ({} files)", files.len()),
            None => format!("all {} files", files.len()),
        };
        let paths = self
            .unstaged
            .iter()
            .filter(|e| files.contains(&e.path))
            .flat_map(|e| std::iter::once(e.path.clone()).chain(e.old_path.clone()))
            .collect::<Vec<_>>();
        if !self.config.confirm_discard {
            self.run_discard_files(&paths);
            return;
        }
        self.prompt = Some((Prompt::ConfirmDiscardFiles { paths, label }, String::new()));
    }

    pub(super) fn stash_push(&mut self) {
        if self.staged.is_empty() && self.unstaged.is_empty() {
            self.error = Some("nothing to stash".to_string());
            return;
        }
        match repo::stash_save(&self.selected, None) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("stash failed: {e}")),
        }
        self.refresh();
    }

    pub(super) fn run_stash_op(&mut self, r: Result<(), twit_core::git2::Error>, what: &str) {
        match r {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.refresh();
    }

    pub(super) fn stage_paths(&mut self, staged: bool, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        let res = if staged {
            repo::unstage(&self.selected, &paths)
        } else {
            repo::stage(&self.selected, &paths)
        };
        match res {
            Ok(()) => {
                self.refresh();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("stage failed: {e}")),
        }
    }
}
