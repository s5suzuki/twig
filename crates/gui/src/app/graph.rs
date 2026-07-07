use super::*;

#[derive(Clone)]
pub enum GraphItem {
    Commit(usize),
    File(usize),
    Folder(String),
}

pub struct GraphMenu {
    pub oid: Oid,
    pub pos: egui::Pos2,
    pub cursor: usize,
}

impl App {
    pub(super) fn clear_commit_selection(&mut self) {
        self.selected_commit = None;
        self.commit_files.clear();
        self.commit_detail.clear();
        self.selected_commit_file = None;
    }

    pub fn select_commit(&mut self, oid: Oid) {
        if self
            .selected_commit
            .as_ref()
            .is_some_and(|(o, _)| *o == oid)
        {
            self.clear_commit_selection();
            self.diff = empty_diff();
            self.diff_ver = self.diff_ver.wrapping_add(1);
            return;
        }
        self.load_commit(oid);
    }

    pub(super) fn load_commit(&mut self, oid: Oid) {
        if oid.is_zero() {
            self.commit_files = self.uncommitted_commit_files();
            self.commit_detail = String::new();
            self.selected_commit = Some((oid, "Uncommitted Changes".to_string()));
        } else {
            let short = oid.to_string();
            let label = short[..7.min(short.len())].to_string();
            self.commit_files = repo::commit_files(&self.selected, oid).unwrap_or_default();
            self.commit_detail = repo::commit_message(&self.selected, oid).unwrap_or_default();
            self.selected_commit = Some((oid, label));
        }
        self.selected_file = None;
        self.selected_commit_file = None;
        self.diff = FileDiff {
            rows: Vec::new(),
            note: Some("Select a file".to_string()),
            conflict: false,
            rename: false,
            binary: false,
        };
        self.diff_ver = self.diff_ver.wrapping_add(1);
    }

    pub(super) fn uncommitted_commit_files(&self) -> Vec<repo::CommitFile> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for e in self.unstaged.iter().chain(self.staged.iter()) {
            if seen.insert(e.path.clone()) {
                out.push(repo::CommitFile {
                    path: e.path.clone(),
                    kind: e.kind,
                });
            }
        }
        out
    }

    pub fn select_commit_file(&mut self, file: String) {
        let Some((oid, _)) = self.selected_commit else {
            return;
        };
        let result = if oid.is_zero() {
            let staged = self.worktree_file_staged(&file);
            let mode = if staged {
                DiffMode::Staged
            } else {
                DiffMode::Unstaged
            };
            repo::file_diff(&self.selected, &file, mode)
        } else {
            repo::commit_file_diff(&self.selected, oid, &file)
        };
        match result {
            Ok(d) => {
                self.diff_sig = repo::hash_rows(&d.rows);
                self.diff = d;
            }
            Err(e) => {
                self.diff_sig = 0;
                self.diff = FileDiff {
                    rows: Vec::new(),
                    note: Some(format!("file diff failed: {e}")),
                    conflict: false,
                    rename: false,
                    binary: false,
                }
            }
        }
        self.selected_file = None;
        self.selected_commit_file = Some(file.clone());
        self.diff_ver = self.diff_ver.wrapping_add(1);
        self.reset_diff_nav();
        self.clamp_diff_nav();
        self.diff_scroll_pending = true;
        self.active_tab = Tab::Diff;
        if oid.is_zero() {
            self.arm_diff_recheck(&file);
        }
    }

    pub(super) fn worktree_file_staged(&self, file: &str) -> bool {
        !self.unstaged.iter().any(|e| e.path == file) && self.staged.iter().any(|e| e.path == file)
    }

    pub fn graph_items(&self) -> Vec<GraphItem> {
        let sel = self.selected_commit.as_ref().map(|(o, _)| *o);
        let mut out = Vec::with_capacity(self.graph.rows.len());
        for (i, row) in self.graph.rows.iter().enumerate() {
            out.push(GraphItem::Commit(i));
            if Some(row.id) == sel {
                for r in repo::commit_file_rows(
                    &self.commit_files,
                    self.config.graph_files_tree,
                    &self.commit_folds,
                ) {
                    match r.kind {
                        repo::CommitRowKind::File(k) => out.push(GraphItem::File(k)),
                        repo::CommitRowKind::Folder { path, .. } => {
                            out.push(GraphItem::Folder(path))
                        }
                    }
                }
            }
        }
        out
    }

    pub fn clamp_graph_cursor(&mut self) {
        let n = self.graph_items().len();
        if n == 0 {
            self.graph_cursor = 0;
        } else if self.graph_cursor >= n {
            self.graph_cursor = n - 1;
        }
    }

    pub(super) fn parent_commit_item(&self, items: &[GraphItem], from: usize) -> Option<usize> {
        (0..=from.min(items.len().saturating_sub(1)))
            .rev()
            .find(|&i| matches!(items[i], GraphItem::Commit(_)))
    }

    pub fn move_graph_cursor(&mut self, delta: isize) {
        let n = self.graph_items().len();
        if n == 0 {
            return;
        }
        let last = (n - 1) as isize;
        let cur = self.graph_cursor.min(n - 1) as isize;
        self.graph_cursor = (cur + delta).clamp(0, last) as usize;
        self.graph_scroll_pending = true;
    }

    pub fn set_graph_cursor(&mut self, idx: usize) {
        let n = self.graph_items().len();
        if n == 0 {
            return;
        }
        self.graph_cursor = idx.min(n - 1);
        self.graph_scroll_pending = true;
    }

    pub fn graph_cursor_bottom(&mut self) {
        let n = self.graph_items().len();
        if n > 0 {
            self.graph_cursor = n - 1;
            self.graph_scroll_pending = true;
        }
    }

    pub fn set_graph_cursor_to_commit(&mut self, oid: Oid) {
        if let Some(idx) = self
            .graph_items()
            .iter()
            .position(|it| matches!(it, GraphItem::Commit(row) if self.graph.rows[*row].id == oid))
        {
            self.graph_cursor = idx;
        }
    }

    pub fn graph_target_commit(&self) -> Option<Oid> {
        let items = self.graph_items();
        let idx = self.graph_cursor.min(items.len().checked_sub(1)?);
        let oid = match items.get(idx)? {
            GraphItem::Commit(r) => Some(self.graph.rows[*r].id),
            GraphItem::File(_) | GraphItem::Folder(_) => {
                match &items[self.parent_commit_item(&items, idx)?] {
                    GraphItem::Commit(r) => Some(self.graph.rows[*r].id),
                    _ => None,
                }
            }
        };
        oid.filter(|o| !o.is_zero())
    }

    pub fn set_graph_cursor_to_file(&mut self, path: &str) {
        if let Some(idx) = self.graph_items().iter().position(|it| {
            matches!(it, GraphItem::File(k) if self.commit_files.get(*k).is_some_and(|f| f.path == path))
        }) {
            self.graph_cursor = idx;
            self.graph_scroll_pending = true;
        }
    }

    pub fn set_graph_cursor_to_folder(&mut self, path: &str) {
        if let Some(idx) = self
            .graph_items()
            .iter()
            .position(|it| matches!(it, GraphItem::Folder(p) if p == path))
        {
            self.graph_cursor = idx;
            self.graph_scroll_pending = true;
        }
    }

    pub fn graph_activate(&mut self) {
        let items = self.graph_items();
        let Some(item) = items.get(self.graph_cursor).cloned() else {
            return;
        };
        match item {
            GraphItem::Commit(row) => {
                let oid = self.graph.rows[row].id;
                let already = self
                    .selected_commit
                    .as_ref()
                    .is_some_and(|(o, _)| *o == oid);
                if !already {
                    self.load_commit(oid);
                    self.set_graph_cursor_to_commit(oid);
                }
            }
            GraphItem::File(k) => {
                if let Some(f) = self.commit_files.get(k) {
                    let path = f.path.clone();
                    self.select_commit_file(path);
                }
            }
            GraphItem::Folder(path) => self.toggle_commit_fold(path),
        }
        self.graph_scroll_pending = true;
    }

    pub fn toggle_commit_fold(&mut self, path: String) {
        if !self.commit_folds.remove(&path) {
            self.commit_folds.insert(path);
        }
    }

    pub fn graph_open_editor(&mut self) {
        let items = self.graph_items();
        if let Some(GraphItem::File(k)) = items.get(self.graph_cursor).cloned()
            && let Some(f) = self.commit_files.get(k)
        {
            let path = f.path.clone();
            self.open_in_editor(&path);
        }
    }

    pub fn graph_collapse(&mut self) {
        let items = self.graph_items();
        let Some(item) = items.get(self.graph_cursor).cloned() else {
            return;
        };
        match item {
            GraphItem::Folder(path) if !self.commit_folds.contains(&path) => {
                self.commit_folds.insert(path);
            }
            GraphItem::File(_) | GraphItem::Folder(_) => {
                if let Some(ci) = self.parent_commit_item(&items, self.graph_cursor) {
                    self.graph_cursor = ci;
                    self.graph_scroll_pending = true;
                }
            }
            GraphItem::Commit(row) => {
                let oid = self.graph.rows[row].id;
                if self
                    .selected_commit
                    .as_ref()
                    .is_some_and(|(o, _)| *o == oid)
                {
                    self.clear_commit_selection();
                    self.diff = empty_diff();
                    self.diff_ver = self.diff_ver.wrapping_add(1);
                }
            }
        }
    }
}
