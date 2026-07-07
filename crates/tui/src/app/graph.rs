use super::*;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GraphItem {
    Commit(usize),
    Msg(usize),
    File(usize),
}

pub(super) fn skip_msg(items: &[GraphItem], i: usize, forward: bool) -> usize {
    let is_msg = |j: usize| matches!(items.get(j), Some(GraphItem::Msg(_)));
    if !is_msg(i) {
        return i;
    }
    let ahead = (i + 1..items.len()).find(|&j| !is_msg(j));
    let behind = (0..i).rev().find(|&j| !is_msg(j));
    if forward {
        ahead.or(behind).unwrap_or(i)
    } else {
        behind.or(ahead).unwrap_or(i)
    }
}

impl TuiApp {
    pub fn graph_items(&self) -> Vec<GraphItem> {
        let mut out = Vec::with_capacity(self.graph.rows.len() + self.commit_files.len());
        for (i, row) in self.graph.rows.iter().enumerate() {
            out.push(GraphItem::Commit(i));
            if self.selected_commit == Some(row.id) {
                if !self.commit_detail.is_empty() {
                    for m in 0..=self.commit_detail.len() {
                        out.push(GraphItem::Msg(m));
                    }
                }
                for k in 0..self.commit_files.len() {
                    out.push(GraphItem::File(k));
                }
            }
        }
        out
    }

    pub fn graph_last(&self) -> usize {
        self.graph_items().len().saturating_sub(1)
    }

    pub(super) fn open_commit_diff(&mut self, oid: Oid) {
        if oid.is_zero() {
            self.open_uncommitted(oid);
            return;
        }
        match repo::commit_diff(&self.selected, oid) {
            Ok(d) => {
                self.diff = d;
                self.selected_commit = Some(oid);
                self.selected_commit_file = None;
                self.commit_files = repo::commit_files(&self.selected, oid).unwrap_or_default();
                self.load_commit_detail(oid);
                self.selected_file = None;
                self.rebuild_highlight("");
                self.diff_nav.reset();
                self.diff_nav.clamp(&self.diff.rows);
                self.diff_scroll = 0;
                self.diff_center = false;
                self.active_tab = Tab::Diff;
                self.focus = Pane::RightTab;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("commit diff failed: {e}")),
        }
    }

    pub(super) fn open_uncommitted(&mut self, oid: Oid) {
        self.commit_files = self.uncommitted_files();
        self.commit_detail.clear();
        self.diff = FileDiff {
            note: Some("Select a file".to_string()),
            ..FileDiff::empty()
        };
        self.selected_commit = Some(oid);
        self.selected_commit_file = None;
        self.selected_file = None;
        self.diff_hl = DiffHighlighter::default();
        self.diff_sig = 0;
        self.diff_nav.reset();
        self.diff_scroll = 0;
        self.diff_center = false;
        self.active_tab = Tab::Diff;
        self.focus = Pane::RightTab;
        self.error = None;
    }

    pub(super) fn uncommitted_files(&self) -> Vec<CommitFile> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for e in self.unstaged.iter().chain(self.staged.iter()) {
            if seen.insert(e.path.clone()) {
                out.push(CommitFile {
                    path: e.path.clone(),
                    kind: e.kind,
                });
            }
        }
        out
    }

    pub(super) fn worktree_file_staged(&self, file: &str) -> bool {
        !self.unstaged.iter().any(|e| e.path == file) && self.staged.iter().any(|e| e.path == file)
    }

    pub(super) fn open_commit_file_diff(&mut self, oid: Oid, path: String) {
        let result = if oid.is_zero() {
            let staged = self.worktree_file_staged(&path);
            repo::file_diff(&self.selected, &path, diff_mode(staged))
        } else {
            repo::commit_file_diff(&self.selected, oid, &path)
        };
        match result {
            Ok(d) => {
                self.diff = d;
                self.selected_commit = Some(oid);
                self.selected_commit_file = Some(path.clone());
                self.load_commit_detail(oid);
                if self.commit_files.is_empty() {
                    self.commit_files = if oid.is_zero() {
                        self.uncommitted_files()
                    } else {
                        repo::commit_files(&self.selected, oid).unwrap_or_default()
                    };
                }
                self.selected_file = None;
                self.rebuild_highlight(&path);
                self.diff_nav.reset();
                self.diff_nav.first_hunk(&self.diff.rows);
                self.diff_scroll = 0;
                self.diff_center = true;
                self.active_tab = Tab::Diff;
                self.focus = Pane::RightTab;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("commit file diff failed: {e}")),
        }
    }

    pub(super) fn load_commit_detail(&mut self, oid: Oid) {
        self.commit_detail = if oid.is_zero() {
            Vec::new()
        } else {
            repo::commit_message(&self.selected, oid)
                .map(|m| {
                    let mut lines: Vec<String> =
                        m.trim_end().lines().skip(1).map(str::to_string).collect();
                    while lines.first().is_some_and(|l| l.trim().is_empty()) {
                        lines.remove(0);
                    }
                    lines
                })
                .unwrap_or_default()
        };
    }

    pub(super) fn collapse_commit(&mut self) {
        self.selected_commit = None;
        self.selected_commit_file = None;
        self.commit_files.clear();
        self.commit_detail.clear();
        self.diff = FileDiff::empty();
        self.diff_hl = DiffHighlighter::default();
        self.diff_sig = 0;
        self.diff_nav.reset();
        self.diff_scroll = 0;
    }

    pub(super) fn graph_keys(&mut self, queue: &mut KeyQueue) {
        let half = (self.graph_view_rows / 2).max(1);
        let actions = self
            .keymap
            .resolve(queue, Context::Graph, &mut self.pending_prefix, |a| {
                matches!(
                    a,
                    Action::GraphDown
                        | Action::GraphUp
                        | Action::GraphTop
                        | Action::GraphBottom
                        | Action::GraphHalfPageDown
                        | Action::GraphHalfPageUp
                        | Action::GraphOpen
                        | Action::GraphCollapse
                        | Action::GraphEditor
                        | Action::GraphReset
                        | Action::GraphCreateBranch
                        | Action::GraphCreateTag
                        | Action::GraphCherryPick
                        | Action::GraphRevert
                        | Action::GraphRebaseOnto
                        | Action::GraphRebaseInteractive
                        | Action::GraphCheckout
                        | Action::GraphRenameBranch
                        | Action::GraphDeleteRef
                        | Action::GraphFetch
                        | Action::GraphPull
                        | Action::GraphPush
                        | Action::GraphForcePush
                )
            });
        for a in actions {
            let items = self.graph_items();
            let last = items.len().saturating_sub(1);
            match a {
                Action::GraphDown => {
                    self.graph_cursor = skip_msg(&items, (self.graph_cursor + 1).min(last), true)
                }
                Action::GraphUp => {
                    self.graph_cursor = skip_msg(&items, self.graph_cursor.saturating_sub(1), false)
                }
                Action::GraphTop => self.graph_cursor = 0,
                Action::GraphBottom => self.graph_cursor = skip_msg(&items, last, false),
                Action::GraphHalfPageDown => {
                    self.graph_cursor = skip_msg(&items, (self.graph_cursor + half).min(last), true)
                }
                Action::GraphHalfPageUp => {
                    self.graph_cursor =
                        skip_msg(&items, self.graph_cursor.saturating_sub(half), false)
                }
                Action::GraphOpen => self.graph_open(),
                Action::GraphCollapse => self.graph_collapse(),
                Action::GraphEditor => self.graph_open_editor(),
                Action::GraphReset => self.graph_op_prompt(a),
                Action::GraphCreateBranch => self.graph_op_prompt(a),
                Action::GraphCreateTag => self.graph_op_prompt(a),
                Action::GraphCherryPick => self.graph_op_prompt(a),
                Action::GraphRevert => self.graph_op_prompt(a),
                Action::GraphRebaseOnto => self.graph_op_prompt(a),
                Action::GraphCheckout => self.graph_op_prompt(a),
                Action::GraphRenameBranch => self.graph_op_prompt(a),
                Action::GraphDeleteRef => self.graph_op_prompt(a),
                Action::GraphRebaseInteractive => self.interactive_rebase(),
                Action::GraphFetch => {
                    let remote = repo::primary_remote(&self.selected);
                    self.start_remote(RemoteKind::Fetch, remote, Vec::new());
                }
                Action::GraphPull => {
                    if !self.busy_with_seq() {
                        let remote = repo::primary_remote(&self.selected);
                        self.start_remote(RemoteKind::Pull, remote, Vec::new());
                    }
                }
                Action::GraphPush => self.push(false),
                Action::GraphForcePush => self.push(true),
                _ => {}
            }
        }
    }

    pub(super) fn graph_target(&self) -> Option<(usize, Oid)> {
        let items = self.graph_items();
        let cursor = self.graph_cursor.min(items.len().checked_sub(1)?);
        let row = match items.get(cursor)? {
            GraphItem::Commit(r) => *r,
            GraphItem::Msg(_) | GraphItem::File(_) => {
                (0..cursor).rev().find_map(|i| match items[i] {
                    GraphItem::Commit(r) => Some(r),
                    _ => None,
                })?
            }
        };
        let gr = &self.graph.rows[row];
        (!gr.is_uncommitted).then_some((row, gr.id))
    }

    pub(super) fn row_refs(&self, row: usize) -> Vec<RefTarget> {
        use twit_core::repo::RefKind;
        self.graph.rows[row]
            .refs
            .iter()
            .filter_map(|r| match r.kind {
                RefKind::LocalBranch if !r.is_head => Some(RefTarget::Branch(r.name.clone())),
                RefKind::RemoteBranch => Some(RefTarget::RemoteBranch(r.name.clone())),
                RefKind::Tag => Some(RefTarget::Tag(r.name.clone())),
                _ => None,
            })
            .collect()
    }

    pub(super) fn graph_open_editor(&mut self) {
        let items = self.graph_items();
        if let Some(GraphItem::File(k)) =
            items.get(self.graph_cursor.min(items.len().saturating_sub(1)))
            && let Some(f) = self.commit_files.get(*k)
        {
            self.pending_editor = Some((self.selected.join(&f.path), None));
        }
    }

    pub(super) fn graph_open(&mut self) {
        let items = self.graph_items();
        match items.get(self.graph_cursor.min(items.len().saturating_sub(1))) {
            Some(GraphItem::Commit(row)) => {
                let row = &self.graph.rows[*row];
                if self.selected_commit == Some(row.id) && self.selected_commit_file.is_none() {
                    self.collapse_commit();
                    return;
                }
                let oid = row.id;
                self.open_commit_diff(oid);
                if self.error.is_none() {
                    self.set_graph_cursor_to_commit(oid);
                    if matches!(self.view_mode, ViewMode::All | ViewMode::Single(View::Main)) {
                        self.active_tab = Tab::Graph;
                    }
                }
            }
            Some(GraphItem::File(k)) => {
                if let (Some(oid), Some(f)) = (self.selected_commit, self.commit_files.get(*k)) {
                    let path = f.path.clone();
                    self.open_commit_file_diff(oid, path);
                    self.pending_focus_jump = self.error.is_none();
                }
            }
            Some(GraphItem::Msg(_)) | None => {}
        }
    }

    pub(super) fn graph_collapse(&mut self) {
        let items = self.graph_items();
        let cursor = self.graph_cursor.min(items.len().saturating_sub(1));
        match items.get(cursor) {
            Some(GraphItem::File(_) | GraphItem::Msg(_)) => {
                if let Some(ci) = (0..=cursor)
                    .rev()
                    .find(|&i| matches!(items[i], GraphItem::Commit(_)))
                {
                    self.graph_cursor = ci;
                }
            }
            Some(GraphItem::Commit(row))
                if self.selected_commit == Some(self.graph.rows[*row].id) =>
            {
                self.collapse_commit();
            }
            _ => {}
        }
    }

    pub(super) fn set_graph_cursor_to_commit(&mut self, oid: Oid) {
        if let Some(idx) = self
            .graph_items()
            .iter()
            .position(|it| matches!(it, GraphItem::Commit(r) if self.graph.rows[*r].id == oid))
        {
            self.graph_cursor = idx;
        }
    }
}
