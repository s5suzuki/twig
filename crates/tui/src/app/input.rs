use super::*;

impl TuiApp {
    pub fn handle_input(&mut self, events: Vec<KeyEvent>) {
        for ev in events {
            if self.quit {
                return;
            }
            if self.prompt.is_some() {
                self.handle_prompt_key(ev);
                continue;
            }
            if self.help_open {
                self.handle_help_key(ev);
                continue;
            }
            if self.settings_open {
                self.handle_settings_key(ev);
                continue;
            }
            if self.editor_focused() {
                self.handle_editor_key(ev);
                continue;
            }
            if ev.kind != KeyEventKind::Release && ev.code == KeyCode::Char('?') {
                self.help_open = true;
                self.help_scroll = 0;
                continue;
            }
            if ev.kind != KeyEventKind::Release && ev.code == KeyCode::Char(',') {
                self.settings_open = true;
                self.settings_cursor = 0;
                continue;
            }
            if let Some(nk) = keys::normalize(&ev) {
                self.handle_key(nk);
            }
        }
    }

    pub fn settings_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "graph_commit_limit",
                self.config.graph_commit_limit.to_string(),
            ),
            (
                "graph_show_author",
                self.config.graph_show_author.to_string(),
            ),
            ("graph_show_date", self.config.graph_show_date.to_string()),
            ("confirm_discard", self.config.confirm_discard.to_string()),
        ]
    }

    pub(super) fn handle_settings_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        let last = self.settings_rows().len() - 1;
        match ev.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(',') => self.settings_open = false,
            KeyCode::Char('j') | KeyCode::Down => {
                self.settings_cursor = (self.settings_cursor + 1).min(last)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_cursor = self.settings_cursor.saturating_sub(1)
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_setting(),
            _ => {}
        }
    }

    pub(super) fn activate_setting(&mut self) {
        match self.settings_cursor {
            0 => {
                self.prompt = Some((
                    Prompt::EditGraphLimit,
                    self.config.graph_commit_limit.to_string(),
                ));
            }
            1 => {
                self.config.graph_show_author = !self.config.graph_show_author;
                self.config.save();
            }
            2 => {
                self.config.graph_show_date = !self.config.graph_show_date;
                self.config.save();
            }
            3 => {
                self.config.confirm_discard = !self.config.confirm_discard;
                self.config.save();
            }
            _ => {}
        }
    }

    pub(super) fn handle_help_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        match ev.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => self.help_open = false,
            KeyCode::Char('j') | KeyCode::Down => self.help_scroll += 1,
            KeyCode::Char('k') | KeyCode::Up => {
                self.help_scroll = self.help_scroll.saturating_sub(1)
            }
            _ => {}
        }
    }

    pub(super) fn handle_key(&mut self, nk: (Modifiers, Key)) {
        let mut queue = KeyQueue(vec![nk]);

        if queue.take(Modifiers::CTRL, Key::C) {
            self.quit = true;
            return;
        }
        if self.seq.is_some() {
            if queue.take(Modifiers::SHIFT, Key::C) {
                self.seq_continue();
                return;
            }
            if queue.take(Modifiers::SHIFT, Key::A) {
                self.prompt = Some((Prompt::ConfirmSeqAbort, String::new()));
                return;
            }
        }
        if self.focus == Pane::Changes && queue.take(Modifiers::NONE, Key::C) {
            self.prompt = Some((Prompt::Commit, String::new()));
            return;
        }
        if self.focus == Pane::Changes && queue.take(Modifiers::NONE, Key::A) {
            self.open_amend_prompt();
            return;
        }
        if self.focus == Pane::Changes && queue.take(Modifiers::NONE, Key::Z) {
            self.stash_push();
            return;
        }

        match self.view_mode {
            ViewMode::All => self.handle_key_all(&mut queue),
            ViewMode::Single(view) => self.handle_key_single(view, &mut queue),
        }
    }

    pub(super) fn handle_key_all(&mut self, queue: &mut KeyQueue) {
        let global = self
            .keymap
            .resolve(queue, Context::Global, &mut self.pending_prefix, |a| {
                matches!(
                    a,
                    Action::FocusLeft
                        | Action::FocusRight
                        | Action::CycleTab
                        | Action::CycleTabFwd
                        | Action::CycleTabBack
                        | Action::OpenSearch
                        | Action::NavBack
                        | Action::NavForward
                        | Action::Quit
                )
            });
        for a in global {
            match a {
                Action::FocusLeft => self.focus_move(-1),
                Action::FocusRight => self.focus_move(1),
                Action::CycleTab | Action::CycleTabFwd => self.cycle_tab(1),
                Action::CycleTabBack => self.cycle_tab(-1),
                Action::OpenSearch => self.open_search_tab(),
                Action::NavBack => {
                    self.nav_go_back();
                }
                Action::NavForward => {
                    self.nav_go_forward();
                }
                Action::Quit => self.quit = true,
                _ => {}
            }
        }

        match self.focus {
            Pane::Sidebar => self.sidebar_keys(queue),
            Pane::Changes => self.changes_keys(queue),
            Pane::RightTab => match self.active_tab {
                Tab::Graph => self.graph_keys(queue),
                Tab::Diff => self.diff_keys(queue),
                Tab::Search => self.search_keys(queue),
                Tab::Editor => {}
            },
        }
    }

    pub(super) fn open_search_tab(&mut self) {
        self.active_tab = Tab::Search;
        self.focus = Pane::RightTab;
        self.prompt = Some((Prompt::SearchQuery, self.search.query.clone()));
    }

    pub(super) fn handle_key_single(&mut self, view: View, queue: &mut KeyQueue) {
        let before = self.selection_snapshot();

        let nav = self
            .keymap
            .resolve(queue, Context::Global, &mut self.pending_prefix, |a| {
                matches!(a, Action::NavBack | Action::NavForward | Action::Quit)
            });
        for a in nav {
            match a {
                Action::NavBack => {
                    self.nav_go_back();
                }
                Action::NavForward => {
                    self.nav_go_forward();
                }
                Action::Quit => self.quit = true,
                _ => {}
            }
        }

        if view == View::Main {
            let global =
                self.keymap
                    .resolve(queue, Context::Global, &mut self.pending_prefix, |a| {
                        matches!(
                            a,
                            Action::CycleTab
                                | Action::CycleTabFwd
                                | Action::CycleTabBack
                                | Action::OpenSearch
                        )
                    });
            for a in global {
                match a {
                    Action::CycleTab | Action::CycleTabFwd => self.cycle_tab(1),
                    Action::CycleTabBack => self.cycle_tab(-1),
                    Action::OpenSearch => self.open_search_tab(),
                    _ => {}
                }
            }
        }

        match view {
            View::Sidebar => self.sidebar_keys(queue),
            View::Changes => self.changes_keys(queue),
            View::Graph => self.graph_keys(queue),
            View::Diff => self.diff_keys(queue),
            View::Main => match self.active_tab {
                Tab::Graph => self.graph_keys(queue),
                Tab::Diff => self.diff_keys(queue),
                Tab::Search => self.search_keys(queue),
                Tab::Editor => {}
            },
        }

        self.focus = fixed_focus(view);
        if self.selection_snapshot() != before {
            self.publish();
        }
    }

    pub(super) fn sidebar_keys(&mut self, queue: &mut KeyQueue) {
        let rows = self.sidebar_rows();
        if rows.is_empty() {
            return;
        }
        let last = rows.len() - 1;
        let half = (self.sidebar_view_rows / 2).max(1);
        let took_init = queue.take(Modifiers::NONE, Key::I);
        let took_update = !took_init && queue.take(Modifiers::NONE, Key::U);
        if took_init || took_update {
            let row = &rows[self.sidebar_cursor.min(last)];
            self.submodule_prompt(row, took_update);
            return;
        }
        let actions =
            self.keymap
                .resolve(queue, Context::Sidebar, &mut self.pending_prefix, |_| true);
        for a in actions {
            match a {
                Action::SidebarDown => self.sidebar_cursor = (self.sidebar_cursor + 1).min(last),
                Action::SidebarUp => self.sidebar_cursor = self.sidebar_cursor.saturating_sub(1),
                Action::SidebarTop => self.sidebar_cursor = 0,
                Action::SidebarBottom => self.sidebar_cursor = last,
                Action::SidebarHalfPageDown => {
                    self.sidebar_cursor = (self.sidebar_cursor + half).min(last)
                }
                Action::SidebarHalfPageUp => {
                    self.sidebar_cursor = self.sidebar_cursor.saturating_sub(half)
                }
                Action::SidebarSelect | Action::SidebarExpand => {
                    let row = &rows[self.sidebar_cursor.min(last)];
                    if row.initialized {
                        self.select_repo(row.path.clone());
                    } else {
                        self.error = Some(format!("{} is not initialized", row.label.trim()));
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn focus_move(&mut self, dir: isize) {
        let order = [Pane::Sidebar, Pane::Changes, Pane::RightTab];
        let cur = order.iter().position(|p| *p == self.focus).unwrap_or(1) as isize;
        let next = (cur + dir).clamp(0, order.len() as isize - 1) as usize;
        self.focus = order[next];
    }

    pub(super) fn cycle_tab(&mut self, dir: isize) {
        if self.focus != Pane::RightTab {
            self.focus = Pane::RightTab;
            return;
        }
        let order = [Tab::Graph, Tab::Diff, Tab::Search, Tab::Editor];
        let cur = order
            .iter()
            .position(|t| *t == self.active_tab)
            .unwrap_or(0) as isize;
        let next = (cur + dir).rem_euclid(order.len() as isize) as usize;
        self.active_tab = order[next];
        if self.active_tab == Tab::Editor {
            self.ensure_editor();
        }
    }
}
