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
    tab: Tab,
    focus: Pane,
    sel: NavSel,
    diff_cursor: usize,
}

impl NavPoint {
    fn same_place(&self, other: &NavPoint) -> bool {
        self.tab == other.tab && self.sel == other.sel
    }
}

impl App {
    pub(super) fn current_nav_point(&self) -> NavPoint {
        let sel = if let Some((path, staged)) = &self.selected_file {
            NavSel::File {
                path: path.clone(),
                staged: *staged,
            }
        } else if let (Some((oid, _)), Some(path)) =
            (&self.selected_commit, &self.selected_commit_file)
        {
            NavSel::CommitFile {
                oid: *oid,
                path: path.clone(),
            }
        } else if let Some((oid, _)) = &self.selected_commit {
            NavSel::Commit { oid: *oid }
        } else {
            NavSel::None
        };
        NavPoint {
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
        match p.sel.clone() {
            NavSel::None => {
                self.selected_file = None;
                self.clear_commit_selection();
                self.diff = empty_diff();
                self.diff_ver = self.diff_ver.wrapping_add(1);
            }
            NavSel::File { path, staged } => {
                self.reset_diff_nav();
                self.load_file_diff(path, staged);
            }
            NavSel::Commit { oid } => self.load_commit(oid),
            NavSel::CommitFile { oid, path } => {
                self.load_commit(oid);
                self.select_commit_file(path);
            }
        }
        self.active_tab = p.tab;
        self.focus = p.focus;
        if matches!(p.sel, NavSel::File { .. } | NavSel::CommitFile { .. }) {
            self.diff_nav.cursor = p.diff_cursor.min(self.diff_last_row());
            self.diff_scroll_pending = true;
        }
        self.nav_current = Some(self.current_nav_point());
    }

    pub fn nav_go_back(&mut self) {
        let Some(prev) = self.nav_back.pop() else {
            return;
        };
        if let Some(cur) = self.nav_current.take() {
            self.nav_fwd.push(cur);
        }
        self.restore_nav(prev);
    }

    pub fn nav_go_forward(&mut self) {
        let Some(next) = self.nav_fwd.pop() else {
            return;
        };
        if let Some(cur) = self.nav_current.take() {
            self.nav_back.push(cur);
        }
        self.restore_nav(next);
    }
}
