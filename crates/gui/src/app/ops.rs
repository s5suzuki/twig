use super::*;

impl App {
    pub fn discard_paths(&mut self, paths: &[String]) {
        if let Err(e) = repo::discard(&self.selected, paths) {
            self.error = Some(format!("Failed to discard changes: {e}"));
        }
        self.after_index_change();
    }

    pub fn stage(&mut self, paths: Vec<String>) {
        if let Err(e) = repo::stage(&self.selected, &paths) {
            self.error = Some(format!("stage failed: {e}"));
        }
        self.after_index_change();
    }

    pub fn unstage(&mut self, paths: Vec<String>) {
        if let Err(e) = repo::unstage(&self.selected, &paths) {
            self.error = Some(format!("unstage failed: {e}"));
        }
        self.after_index_change();
    }

    pub fn do_commit(&mut self) {
        if self.commit_msg.trim().is_empty() {
            self.error = Some("Commit message is empty".to_string());
            return;
        }
        if self.amend_mode {
            if !self.can_amend() {
                self.error = Some("Cannot amend now".to_string());
                return;
            }
            if repo::head_is_pushed(&self.selected) {
                self.confirm_amend = true;
            } else {
                self.run_amend();
            }
            return;
        }
        match repo::commit(&self.selected, self.commit_msg.trim()) {
            Ok(()) => {
                self.commit_msg.clear();
                self.selected_file = None;
                self.clear_commit_selection();
                self.diff = empty_diff();
                self.auto_stage_pointer();
                self.reload();
            }
            Err(e) => self.error = Some(format!("commit failed: {e}")),
        }
    }

    pub(super) fn auto_stage_pointer(&mut self) {
        let parent = self
            .root
            .as_ref()
            .and_then(|root| repo::find_submodule_parent(root, &self.selected));
        if let Some((parent_path, name)) = parent
            && let Err(e) = repo::stage_submodule_pointer(&parent_path, &name)
        {
            self.error = Some(format!("stage submodule pointer failed: {e}"));
        }
    }

    pub fn can_amend(&self) -> bool {
        self.seq.is_none() && repo::head_has_commit(&self.selected)
    }

    pub fn set_amend_mode(&mut self, on: bool) {
        if on == self.amend_mode {
            return;
        }
        if on {
            if !self.can_amend() {
                self.error = Some("Nothing to amend".to_string());
                return;
            }
            self.saved_commit_msg = Some(std::mem::take(&mut self.commit_msg));
            self.commit_msg = repo::head_message(&self.selected)
                .map(|m| m.trim_end().to_string())
                .unwrap_or_default();
            self.amend_mode = true;
        } else {
            self.commit_msg = self.saved_commit_msg.take().unwrap_or_default();
            self.amend_mode = false;
        }
    }

    pub(super) fn reset_amend_mode(&mut self) {
        self.amend_mode = false;
        self.saved_commit_msg = None;
        self.confirm_amend = false;
    }

    pub fn run_amend(&mut self) {
        self.confirm_amend = false;
        match repo::amend(&self.selected, Some(self.commit_msg.trim())) {
            Ok(_) => {
                self.commit_msg.clear();
                self.saved_commit_msg = None;
                self.amend_mode = false;
                self.selected_file = None;
                self.clear_commit_selection();
                self.diff = empty_diff();
                self.auto_stage_pointer();
                self.reload();
            }
            Err(e) => self.error = Some(format!("amend failed: {e}")),
        }
    }

    pub fn begin_amend_from_graph(&mut self) {
        self.focus = Pane::Changes;
        self.set_amend_mode(true);
    }

    pub fn stash_push(&mut self) {
        if self.staged.is_empty() && self.unstaged.is_empty() {
            self.error = Some("Nothing to stash".to_string());
            return;
        }
        match repo::stash_save(&self.selected, None) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("stash failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn stash_pop(&mut self, index: usize) {
        self.run_stash(repo::stash_pop(&self.selected, index), "stash pop");
    }

    pub fn stash_apply(&mut self, index: usize) {
        self.run_stash(repo::stash_apply(&self.selected, index), "stash apply");
    }

    pub fn stash_drop(&mut self, index: usize) {
        self.run_stash(repo::stash_drop(&self.selected, index), "stash drop");
    }

    pub(super) fn run_stash(&mut self, r: Result<(), git2::Error>, what: &str) {
        match r {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub(super) fn after_index_change(&mut self) {
        self.reload();
        if let Some((path, was_staged)) = self.selected_file.clone() {
            let in_unstaged = self.unstaged.iter().any(|e| e.path == path);
            let in_staged = self.staged.iter().any(|e| e.path == path);
            let side = if in_unstaged && in_staged {
                Some(was_staged)
            } else if in_unstaged {
                Some(false)
            } else if in_staged {
                Some(true)
            } else {
                None
            };
            match side {
                Some(staged) => self.load_file_diff(path, staged),
                None => {
                    self.selected_file = None;
                    self.diff = empty_diff();
                }
            }
        }
        self.refresh_uncommitted_diff();
    }
}
