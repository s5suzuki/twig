use super::*;

#[derive(Clone)]
pub struct DiscardReq {
    pub paths: Vec<String>,
    pub label: String,
}

pub(super) fn empty_diff() -> FileDiff {
    FileDiff::empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_cmd(dir: &std::path::Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    fn advance(ctx: &egui::Context, t: f64) {
        let input = egui::RawInput {
            time: Some(t),
            ..Default::default()
        };
        ctx.begin_pass(input);
        let _ = ctx.end_pass();
    }

    #[test]
    fn transient_empty_new_file_diff_recovers_via_recheck() {
        let tmp = std::env::temp_dir().join(format!("twig-recheck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        run_cmd(&tmp, &["init", "-q"]);
        run_cmd(&tmp, &["config", "user.email", "a@b.c"]);
        run_cmd(&tmp, &["config", "user.name", "a"]);
        std::fs::write(tmp.join("base.txt"), "base\n").unwrap();
        run_cmd(&tmp, &["add", "base.txt"]);
        run_cmd(&tmp, &["commit", "-qm", "init"]);

        let content = "line1\nline2\nline3\n";
        std::fs::write(tmp.join("new.txt"), content).unwrap();

        let mut app = App::new(tmp.clone());
        assert!(
            app.worktree_file_changed("new.txt"),
            "untracked file must be seen as changed"
        );

        app.load_file_diff("new.txt".to_string(), false);
        assert!(!app.diff.rows.is_empty(), "settled file must diff");
        assert_eq!(app.diff_recheck, 0, "settled diff must not arm recheck");

        std::fs::write(tmp.join("new.txt"), "").unwrap();
        app.load_file_diff("new.txt".to_string(), false);
        assert!(app.diff.rows.is_empty());
        assert_eq!(app.diff.note.as_deref(), Some("(no changes)"));
        assert_eq!(
            app.diff_recheck, DIFF_RECHECK_TRIES,
            "transient empty diff must arm recheck"
        );

        std::fs::write(tmp.join("new.txt"), content).unwrap();
        let ctx = egui::Context::default();
        let mut t = 1.0;
        for _ in 0..30 {
            advance(&ctx, t);
            app.poll_diff_recheck(&ctx);
            if app.diff_recheck == 0 {
                break;
            }
            t += DIFF_RECHECK_INTERVAL * 2.0;
        }
        assert_eq!(app.diff_recheck, 0, "recheck must terminate");
        assert!(
            !app.diff.rows.is_empty(),
            "recheck must recover the real diff, not the transient empty"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn binary_new_file_does_not_arm_recheck() {
        let tmp = std::env::temp_dir().join(format!("twig-recheck-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        run_cmd(&tmp, &["init", "-q"]);
        run_cmd(&tmp, &["config", "user.email", "a@b.c"]);
        run_cmd(&tmp, &["config", "user.name", "a"]);
        std::fs::write(tmp.join("base.txt"), "base\n").unwrap();
        run_cmd(&tmp, &["add", "base.txt"]);
        run_cmd(&tmp, &["commit", "-qm", "init"]);

        std::fs::write(tmp.join("blob.bin"), b"x\x00y\x00z\x00").unwrap();

        let mut app = App::new(tmp.clone());
        app.load_file_diff("blob.bin".to_string(), false);
        assert!(app.diff.binary, "binary flag must be set");
        assert_eq!(app.diff.note.as_deref(), Some("(binary)"));
        assert_eq!(app.diff_recheck, 0, "binary file must not arm recheck");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

impl App {
    pub(super) fn load_file_diff(&mut self, file: String, staged: bool) {
        let mode = if staged {
            DiffMode::Staged
        } else {
            DiffMode::Unstaged
        };
        let prev = self.selected_file.clone();
        match repo::file_diff(&self.selected, &file, mode) {
            Ok(d) => {
                let sig = repo::hash_rows(&d.rows);
                let unchanged = prev.as_ref() == Some(&(file.clone(), staged))
                    && sig == self.diff_sig
                    && !d.rows.is_empty();
                self.diff = d;
                self.diff_sig = sig;
                if !unchanged {
                    self.diff_ver = self.diff_ver.wrapping_add(1);
                    self.find.invalidate();
                }
            }
            Err(e) => {
                self.diff = FileDiff {
                    rows: Vec::new(),
                    note: Some(format!("diff failed: {e}")),
                    conflict: false,
                    rename: false,
                    binary: false,
                };
                self.diff_sig = 0;
                self.diff_ver = self.diff_ver.wrapping_add(1);
                self.find.invalidate();
            }
        }
        self.selected_file = Some((file.clone(), staged));
        self.clear_commit_selection();

        self.clamp_diff_nav();
        self.arm_diff_recheck(&file);
    }

    pub fn diff_version(&self) -> u64 {
        self.diff_ver
    }

    pub(super) fn diff_path(&self) -> Option<String> {
        self.selected_file
            .as_ref()
            .map(|(p, _)| p.clone())
            .or_else(|| self.selected_commit_file.clone())
    }

    pub fn ensure_diff_highlight(&mut self, dark: bool) {
        let sig = (self.diff_ver, dark);
        if self.diff_hl_sig == Some(sig) {
            return;
        }
        self.diff_hl_sig = Some(sig);
        self.diff_hl = match self.diff_path() {
            Some(path) if !self.diff.rows.is_empty() => {
                twit_core::highlight::DiffHighlighter::new(&path, &self.diff.rows, dark)
            }
            _ => twit_core::highlight::DiffHighlighter::default(),
        };
    }

    pub(super) fn reset_diff_nav(&mut self) {
        self.diff_nav.reset();
        self.diff_scroll_pending = false;
        self.diff_visible = None;
    }

    pub(super) fn clamp_diff_nav(&mut self) {
        self.diff_nav.clamp(&self.diff.rows);
    }

    pub fn diff_last_row(&self) -> usize {
        diffnav::last_row(&self.diff.rows)
    }

    pub fn move_diff_cursor(&mut self, delta: isize) {
        let last = self.diff_last_row();
        let cur = self.diff_nav.cursor.min(last);

        if !self.diff_scrolled_prev
            && let Some((vt, vb)) = self.diff_visible
        {
            if cur < vt {
                self.diff_nav.cursor = vt;
                self.diff_scroll_pending = true;
                return;
            }
            if cur > vb {
                self.diff_nav.cursor = vb;
                self.diff_scroll_pending = true;
                return;
            }
        }
        self.diff_nav.step(&self.diff.rows, delta);
        self.diff_scroll_pending = true;
    }

    pub fn set_diff_cursor(&mut self, row: usize) {
        self.diff_nav.set_cursor(&self.diff.rows, row);
        self.diff_scroll_pending = true;
    }

    pub fn jump_hunk(&mut self, forward: bool) {
        if self.diff_nav.jump_hunk(&self.diff.rows, forward) {
            self.diff_scroll_pending = true;
            self.diff_scroll_center = true;
        }
    }

    pub fn toggle_diff_visual(&mut self) {
        self.diff_nav.toggle_visual();
    }

    pub fn diff_highlight(&self) -> Option<(usize, usize)> {
        self.diff_nav.highlight(&self.diff.rows)
    }

    pub(super) fn diff_action_range(&self) -> Option<(usize, usize)> {
        self.diff_nav.action_range(&self.diff.rows)
    }

    pub fn diff_selection_text(&self) -> Option<String> {
        self.diff_nav.selection_text(&self.diff.rows)
    }

    pub fn apply_line_selection(&mut self) {
        if self.diff.rename {
            return;
        }
        let Some((path, staged)) = self.selected_file.clone() else {
            return;
        };
        let Some((lo, hi)) = self.diff_action_range() else {
            return;
        };
        if let Err(e) = repo::apply_partial(&self.selected, &path, &self.diff.rows, lo, hi, staged)
        {
            self.error = Some(format!("partial stage failed: {e}"));
        }
        self.diff_nav.anchor = None;
        self.after_index_change();
    }

    pub fn request_discard_selection(&mut self) {
        if self.diff.rename || self.diff.conflict {
            return;
        }
        let Some((path, staged)) = self.selected_file.clone() else {
            return;
        };
        if staged {
            return;
        }
        let Some((lo, hi)) = self.diff_action_range() else {
            return;
        };
        if self.config.confirm_discard {
            self.confirm_discard_range = Some((path, lo, hi));
        } else {
            self.discard_line_selection(&path, lo, hi);
        }
    }

    pub fn discard_line_selection(&mut self, path: &str, lo: usize, hi: usize) {
        if let Err(e) = repo::discard_partial(&self.selected, path, &self.diff.rows, lo, hi) {
            self.error = Some(format!("partial discard failed: {e}"));
        }
        self.diff_nav.anchor = None;
        self.after_index_change();
    }

    pub fn select_file(&mut self, file: String, staged: bool) {
        self.reset_diff_nav();
        self.load_file_diff(file, staged);
        self.cursor_to_first_hunk();
        self.active_tab = Tab::Diff;
        self.focus = Pane::RightTab;
    }

    pub(super) fn cursor_to_first_hunk(&mut self) {
        if self.diff_nav.first_hunk(&self.diff.rows) {
            self.diff_scroll_center = true;
        }
        self.diff_scroll_pending = true;
    }

    pub fn hunk_index_at_cursor(&self) -> Option<usize> {
        let rows = &self.diff.rows;
        let cursor = self.diff_nav.cursor.min(rows.len().checked_sub(1)?);
        (0..=cursor).rev().find_map(|i| match rows[i] {
            repo::DiffRow::Hunk { index, .. } => Some(index),
            _ => None,
        })
    }

    pub fn toggle_hunk(&mut self, hunk_index: usize) {
        if self.diff.rename {
            return;
        }
        let Some((path, staged)) = self.selected_file.clone() else {
            return;
        };
        let res = if staged {
            repo::unstage_hunk(&self.selected, &path, hunk_index)
        } else {
            repo::stage_hunk(&self.selected, &path, hunk_index)
        };
        if let Err(e) = res {
            self.error = Some(format!("hunk op failed: {e}"));
        }
        self.after_index_change();
    }

    pub fn scroll_diff(&mut self, fraction: f32, down: bool) {
        let visible_rows = match self.diff_visible {
            Some((top, bottom)) => bottom.saturating_sub(top) + 1,
            None => 20,
        };
        self.diff_nav
            .scroll(&self.diff.rows, visible_rows, fraction, down);
        self.diff_scroll_pending = true;
    }

    pub(super) fn worktree_file_changed(&self, file: &str) -> bool {
        self.unstaged.iter().any(|e| e.path == file) || self.staged.iter().any(|e| e.path == file)
    }

    pub(super) fn arm_diff_recheck(&mut self, file: &str) {
        if self.in_recheck {
            return;
        }
        let transient = self.diff.rows.is_empty()
            && !self.diff.binary
            && self.diff.note.as_deref() == Some("(no changes)")
            && self.worktree_file_changed(file);
        if transient {
            self.diff_recheck = DIFF_RECHECK_TRIES;
            self.diff_recheck_at = 0.0;
        } else {
            self.diff_recheck = 0;
        }
    }

    pub fn poll_diff_recheck(&mut self, ctx: &egui::Context) {
        if self.diff_recheck == 0 {
            return;
        }
        let visible = self.repaint_gate.load(Ordering::Relaxed);
        let interval = std::time::Duration::from_secs_f64(DIFF_RECHECK_INTERVAL);
        let now = ctx.input(|i| i.time);
        if self.diff_recheck_at == 0.0 {
            self.diff_recheck_at = now + DIFF_RECHECK_INTERVAL;
        }
        if now < self.diff_recheck_at {
            if visible {
                ctx.request_repaint_after(interval);
            }
            return;
        }

        self.diff_recheck -= 1;
        self.diff_recheck_at = now + DIFF_RECHECK_INTERVAL;
        self.in_recheck = true;
        if let Some((file, staged)) = self.selected_file.clone() {
            self.load_file_diff(file, staged);
        } else if self
            .selected_commit
            .as_ref()
            .is_some_and(|(o, _)| o.is_zero())
        {
            self.refresh_uncommitted_diff();
        }
        self.in_recheck = false;

        if !self.diff.rows.is_empty() {
            self.diff_recheck = 0;
        }
        if self.diff_recheck > 0 && visible {
            ctx.request_repaint_after(interval);
        }
    }

    pub(super) fn refresh_uncommitted_diff(&mut self) {
        if !self
            .selected_commit
            .as_ref()
            .is_some_and(|(o, _)| o.is_zero())
        {
            return;
        }
        let Some(file) = self.selected_commit_file.clone() else {
            return;
        };
        let mode = if self.worktree_file_staged(&file) {
            DiffMode::Staged
        } else {
            DiffMode::Unstaged
        };
        if let Ok(d) = repo::file_diff(&self.selected, &file, mode) {
            let sig = repo::hash_rows(&d.rows);
            let changed = sig != self.diff_sig || d.rows.is_empty();
            self.diff = d;
            self.diff_sig = sig;
            if changed {
                self.diff_ver = self.diff_ver.wrapping_add(1);
                self.find.invalidate();
            }
            self.arm_diff_recheck(&file);
        }
    }
}
