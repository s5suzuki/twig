use super::*;

#[derive(Clone, PartialEq)]
pub(super) enum NavSel {
    None,
    File { path: String, staged: bool },
    Commit { oid: Oid },
    CommitFile { oid: Oid, path: String },
}

#[derive(Clone)]
pub(super) struct NavPoint {
    repo: PathBuf,
    tab: Tab,
    focus: Pane,
    sel: NavSel,
    diff_cursor: usize,
}

impl NavPoint {
    fn same_place(&self, other: &NavPoint) -> bool {
        self.repo == other.repo && self.tab == other.tab && self.sel == other.sel
    }
}

impl TuiApp {
    pub(super) fn current_nav_point(&self) -> NavPoint {
        let sel = if let Some((path, staged)) = &self.selected_file {
            NavSel::File {
                path: path.clone(),
                staged: *staged,
            }
        } else if let (Some(oid), Some(path)) = (&self.selected_commit, &self.selected_commit_file)
        {
            NavSel::CommitFile {
                oid: *oid,
                path: path.clone(),
            }
        } else if let Some(oid) = &self.selected_commit {
            NavSel::Commit { oid: *oid }
        } else {
            NavSel::None
        };
        NavPoint {
            repo: self.selected.clone(),
            tab: self.active_tab,
            focus: self.focus,
            sel,
            diff_cursor: self.diff_nav.cursor,
        }
    }

    pub fn track_nav(&mut self) {
        let loc = self.current_nav_point();
        if let Some(prev) = &self.nav_current
            && !prev.same_place(&loc)
        {
            self.nav_back.push(prev.clone());
            if self.nav_back.len() > NAV_HISTORY_MAX {
                self.nav_back.remove(0);
            }
            self.nav_fwd.clear();
        }
        self.nav_current = Some(loc);
    }

    pub(super) fn restore_nav(&mut self, p: NavPoint) {
        if self.selected != p.repo {
            self.select_repo(p.repo.clone());
        }
        match p.sel.clone() {
            NavSel::None => {
                self.selected_file = None;
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
            NavSel::File { path, staged } => self.open_file_diff(path, staged),
            NavSel::Commit { oid } => self.open_commit_diff(oid),
            NavSel::CommitFile { oid, path } => self.open_commit_file_diff(oid, path),
        }
        self.active_tab = p.tab;
        self.focus = p.focus;
        if matches!(p.sel, NavSel::File { .. } | NavSel::CommitFile { .. }) {
            let last = self.diff.rows.len().saturating_sub(1);
            self.diff_nav.cursor = p.diff_cursor.min(last);
        }
        self.nav_current = Some(self.current_nav_point());
    }

    pub fn nav_go_back(&mut self) -> bool {
        let Some(prev) = self.nav_back.pop() else {
            return false;
        };
        if let Some(cur) = self.nav_current.take() {
            self.nav_fwd.push(cur);
        }
        self.restore_nav(prev);
        true
    }

    pub fn nav_go_forward(&mut self) -> bool {
        let Some(next) = self.nav_fwd.pop() else {
            return false;
        };
        if let Some(cur) = self.nav_current.take() {
            self.nav_back.push(cur);
        }
        self.restore_nav(next);
        true
    }
}
