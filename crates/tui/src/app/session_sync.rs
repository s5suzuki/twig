use super::*;

pub(super) type Snapshot = (
    PathBuf,
    Option<(String, bool)>,
    Option<Oid>,
    Option<String>,
    Tab,
);

impl TuiApp {
    pub(super) fn selection_snapshot(&self) -> Snapshot {
        (
            self.selected.clone(),
            self.selected_file.clone(),
            self.selected_commit,
            self.selected_commit_file.clone(),
            self.active_tab,
        )
    }

    pub(super) fn publish(&mut self) {
        let repo = self.selected.clone();
        let file = self.selected_file.clone();
        let commit = self.selected_commit.map(|o| o.to_string());
        let commit_file = self.selected_commit_file.clone();
        let tab = self.active_tab;
        if let Some(sess) = self.session.as_mut() {
            sess.publish(|st| {
                st.selected_repo = repo;
                st.selected_file = file;
                st.selected_commit = commit;
                st.selected_commit_file = commit_file;
                st.active_tab = tab;
            });
        }
    }

    pub fn apply_shared(&mut self, st: &SharedState) {
        if st.selected_repo != self.selected && !st.selected_repo.as_os_str().is_empty() {
            self.select_repo(st.selected_repo.clone());
        }

        if matches!(self.view_mode, ViewMode::Single(View::Main | View::Diff)) {
            let cur_commit = self.selected_commit.map(|o| o.to_string());
            if let Some(hex) = &st.selected_commit {
                if (st.selected_commit != cur_commit
                    || st.selected_commit_file != self.selected_commit_file)
                    && let Ok(oid) = Oid::from_str(hex)
                {
                    match &st.selected_commit_file {
                        Some(path) => self.open_commit_file_diff(oid, path.clone()),
                        None => self.open_commit_diff(oid),
                    }
                }
            } else if let Some((path, staged)) = &st.selected_file {
                if st.selected_file != self.selected_file {
                    self.open_file_diff(path.clone(), *staged);
                }
            } else if self.selected_file.is_some() || self.selected_commit.is_some() {
                self.selected_file = None;
                self.selected_commit = None;
                self.selected_commit_file = None;
                self.commit_files.clear();
                self.commit_detail.clear();
                self.diff = FileDiff::empty();
                self.diff_nav.reset();
                self.diff_scroll = 0;
            }
        }
        if self.view_mode == ViewMode::Single(View::Main) {
            self.active_tab = st.active_tab;
            match self.editor_seq_seen {
                None => self.editor_seq_seen = Some(st.editor_seq),
                Some(prev) if st.editor_seq > prev => {
                    self.editor_seq_seen = Some(st.editor_seq);
                    if let Some(file) = st.editor_file.clone() {
                        self.open_in_embedded(Path::new(&file), st.editor_line);
                    }
                }
                _ => {}
            }
        }
        if let ViewMode::Single(view) = self.view_mode {
            self.focus = fixed_focus(view);
        }
    }

    pub fn sync_session(&mut self) -> bool {
        let Some(sess) = self.session.as_mut() else {
            return false;
        };
        let tick = sess.tick();
        let mut dirty = false;
        if let Some(state) = tick.changed {
            if state.quit {
                self.quit = true;
                self.quit_broadcast = false;
                return true;
            }
            self.apply_shared(&state);
            dirty = true;
        }
        if tick.quit && !self.quit {
            self.quit = true;
            dirty = true;
        }
        dirty
    }
}
