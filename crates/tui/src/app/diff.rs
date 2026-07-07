use super::*;

pub(super) fn fold_ascii(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_lowercase()).collect()
}

pub fn find_ranges(query: &str, text: &str) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let folded_text = fold_ascii(text);
    let folded_query = fold_ascii(query);
    folded_text
        .match_indices(&folded_query)
        .map(|(i, m)| i..i + m.len())
        .collect()
}

pub(super) fn row_contains(row: &repo::DiffRow, query: &str) -> bool {
    match row {
        repo::DiffRow::Line { left, right, .. } => {
            left.as_deref()
                .is_some_and(|t| !find_ranges(query, t).is_empty())
                || right
                    .as_deref()
                    .is_some_and(|t| !find_ranges(query, t).is_empty())
        }
        _ => false,
    }
}

pub(super) fn diff_mode(staged: bool) -> repo::DiffMode {
    if staged {
        repo::DiffMode::Staged
    } else {
        repo::DiffMode::Unstaged
    }
}

impl TuiApp {
    pub(super) fn open_file_diff(&mut self, path: String, staged: bool) {
        match repo::file_diff(&self.selected, &path, diff_mode(staged)) {
            Ok(d) => {
                self.diff = d;
                self.selected_file = Some((path.clone(), staged));
                self.selected_commit = None;
                self.selected_commit_file = None;
                self.commit_files.clear();
                self.commit_detail.clear();
                self.rebuild_highlight(&path);
                self.diff_nav.reset();
                self.diff_nav.first_hunk(&self.diff.rows);
                self.diff_scroll = 0;
                self.diff_center = true;
                self.active_tab = Tab::Diff;
                self.focus = Pane::RightTab;
                self.error = None;
                self.arm_diff_recheck();
            }
            Err(e) => self.error = Some(format!("diff failed: {e}")),
        }
    }

    pub(super) fn reload_file_diff(&mut self, path: &str, staged: bool) {
        match repo::file_diff(&self.selected, path, diff_mode(staged)) {
            Ok(d) => {
                let sig = repo::hash_rows(&d.rows);
                let changed = sig != self.diff_sig;
                self.diff = d;
                if changed {
                    self.diff_sig = sig;
                    self.diff_hl = DiffHighlighter::new(path, &self.diff.rows, true);
                }
                self.diff_nav.clamp(&self.diff.rows);
            }
            Err(_) => {
                self.selected_file = None;
                self.diff = FileDiff::empty();
            }
        }
    }

    pub(super) fn rebuild_highlight(&mut self, path: &str) {
        self.diff_sig = repo::hash_rows(&self.diff.rows);
        self.diff_hl = DiffHighlighter::new(path, &self.diff.rows, true);
    }

    pub(super) fn diff_keys(&mut self, queue: &mut KeyQueue) {
        if self.diff_find.is_some() {
            if queue.take(Modifiers::NONE, Key::N) {
                self.jump_find(true);
                return;
            }
            if queue.take(Modifiers::SHIFT, Key::N) {
                self.jump_find(false);
                return;
            }
        }
        let actions = self
            .keymap
            .resolve(queue, Context::Diff, &mut self.pending_prefix, |_| true);
        let rows_len = self.diff.rows.len();
        for a in actions {
            match a {
                Action::DiffDown => self.diff_nav.step(&self.diff.rows, 1),
                Action::DiffUp => self.diff_nav.step(&self.diff.rows, -1),
                Action::DiffTop => self.diff_nav.set_cursor(&self.diff.rows, 0),
                Action::DiffBottom => self
                    .diff_nav
                    .set_cursor(&self.diff.rows, rows_len.saturating_sub(1)),
                Action::DiffNextHunk => {
                    if self.diff_nav.jump_hunk(&self.diff.rows, true) {
                        self.diff_center = true;
                    }
                }
                Action::DiffPrevHunk => {
                    if self.diff_nav.jump_hunk(&self.diff.rows, false) {
                        self.diff_center = true;
                    }
                }
                Action::DiffToggleVisual => self.diff_nav.toggle_visual(),
                Action::DiffClearVisual => {
                    self.diff_nav.anchor = None;
                    self.diff_find = None;
                }
                Action::DiffFind => {
                    let current = self.diff_find.clone().unwrap_or_default();
                    self.prompt = Some((Prompt::DiffFind, current));
                }
                Action::DiffHalfPageDown => {
                    self.diff_nav
                        .scroll(&self.diff.rows, self.diff_view_rows, 0.5, true)
                }
                Action::DiffHalfPageUp => {
                    self.diff_nav
                        .scroll(&self.diff.rows, self.diff_view_rows, 0.5, false)
                }
                Action::DiffPageDown => {
                    self.diff_nav
                        .scroll(&self.diff.rows, self.diff_view_rows, 1.0, true)
                }
                Action::DiffPageUp => {
                    self.diff_nav
                        .scroll(&self.diff.rows, self.diff_view_rows, 1.0, false)
                }
                Action::DiffCopySelection => {
                    if let Some(text) = self.diff_nav.selection_text(&self.diff.rows) {
                        self.pending_copy = Some(text);
                        self.diff_nav.anchor = None;
                    }
                }
                Action::DiffEditor => {
                    if let Some((path, _)) = &self.selected_file {
                        self.pending_editor = Some((self.selected.join(path), None));
                    }
                }
                Action::DiffStageSelection => self.apply_line_selection(false),
                Action::DiffUnstageSelection => self.apply_line_selection(true),
                Action::DiffDiscardSelection => self.request_discard_selection(),
                Action::DiffStageHunk => self.apply_hunk_at_cursor(false),
                Action::DiffUnstageHunk => self.apply_hunk_at_cursor(true),
                _ => {}
            }
        }
    }

    pub(super) fn worktree_diff_target(&self, want_staged: bool) -> Option<String> {
        if self.diff.conflict || self.diff.rename {
            return None;
        }
        match &self.selected_file {
            Some((path, staged)) if *staged == want_staged => Some(path.clone()),
            _ => None,
        }
    }

    pub(super) fn apply_line_selection(&mut self, unstage: bool) {
        let Some(path) = self.worktree_diff_target(unstage) else {
            return;
        };
        let Some((lo, hi)) = self.diff_nav.action_range(&self.diff.rows) else {
            return;
        };
        if let Err(e) = repo::apply_partial(&self.selected, &path, &self.diff.rows, lo, hi, unstage)
        {
            self.error = Some(format!("partial stage failed: {e}"));
        } else {
            self.error = None;
        }
        self.diff_nav.anchor = None;
        self.refresh();
    }

    pub(super) fn request_discard_selection(&mut self) {
        let Some(path) = self.worktree_diff_target(false) else {
            return;
        };
        let Some((lo, hi)) = self.diff_nav.action_range(&self.diff.rows) else {
            return;
        };
        if !self.config.confirm_discard {
            self.run_discard_lines(&path, lo, hi);
            return;
        }
        self.prompt = Some((Prompt::ConfirmDiscardLines { path, lo, hi }, String::new()));
    }

    pub(super) fn apply_hunk_at_cursor(&mut self, unstage: bool) {
        let Some(path) = self.worktree_diff_target(unstage) else {
            return;
        };
        let Some((lo, hi)) = self.hunk_range_at_cursor() else {
            return;
        };
        if let Err(e) = repo::apply_partial(&self.selected, &path, &self.diff.rows, lo, hi, unstage)
        {
            self.error = Some(format!("hunk stage failed: {e}"));
        } else {
            self.error = None;
        }
        self.diff_nav.anchor = None;
        self.refresh();
    }

    pub(super) fn hunk_range_at_cursor(&self) -> Option<(usize, usize)> {
        let rows = &self.diff.rows;
        if rows.is_empty() {
            return None;
        }
        let cursor = self.diff_nav.cursor.min(rows.len() - 1);
        let is_boundary = |r: &repo::DiffRow| {
            matches!(r, repo::DiffRow::Hunk { .. } | repo::DiffRow::FileHeader(_))
        };
        if rows.iter().any(|r| matches!(r, repo::DiffRow::Hunk { .. })) {
            let lo = (0..=cursor).rev().find(|&i| is_boundary(&rows[i]))? + 1;
            let hi = (cursor + 1..rows.len())
                .find(|&i| is_boundary(&rows[i]))
                .unwrap_or(rows.len())
                - 1;
            (lo <= hi).then_some((lo, hi))
        } else {
            Some((0, rows.len() - 1))
        }
    }

    pub(super) fn jump_find(&mut self, forward: bool) {
        let Some(query) = self.diff_find.clone() else {
            return;
        };
        let rows = &self.diff.rows;
        if rows.is_empty() {
            return;
        }
        let n = rows.len();
        let start = self.diff_nav.cursor.min(n - 1);
        for step in 1..=n {
            let i = if forward {
                (start + step) % n
            } else {
                (start + n - step % n) % n
            };
            if row_contains(&rows[i], &query) {
                self.diff_nav.set_cursor(rows, i);
                self.diff_center = true;
                return;
            }
        }
        self.error = Some(format!("no match: {query}"));
    }

    pub(super) fn arm_diff_recheck(&mut self) {
        let transient = self.diff.rows.is_empty()
            && !self.diff.binary
            && self.diff.note.as_deref() == Some("(no changes)")
            && self
                .selected_file
                .as_ref()
                .is_some_and(|(p, _)| self.worktree_file_changed(p));
        if transient {
            self.diff_recheck = 5;
            self.diff_recheck_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
        } else {
            self.diff_recheck = 0;
            self.diff_recheck_at = None;
        }
    }

    pub(super) fn worktree_file_changed(&self, file: &str) -> bool {
        self.unstaged.iter().any(|e| e.path == file) || self.staged.iter().any(|e| e.path == file)
    }

    pub fn poll_diff_recheck(&mut self) -> bool {
        if self.diff_recheck == 0 {
            return false;
        }
        let Some(at) = self.diff_recheck_at else {
            return false;
        };
        if std::time::Instant::now() < at {
            return false;
        }
        self.diff_recheck -= 1;
        self.diff_recheck_at =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
        let Some((path, staged)) = self.selected_file.clone() else {
            self.diff_recheck = 0;
            return false;
        };
        self.reload_file_diff(&path, staged);
        if !self.diff.rows.is_empty() || self.diff_recheck == 0 {
            self.diff_recheck = 0;
            self.diff_recheck_at = None;
        }
        true
    }
}
