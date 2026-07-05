use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};
use twig_core::config::Config;
use twig_core::diffnav::DiffNavState;
use twig_core::git2::Oid;
use twig_core::highlight::DiffHighlighter;
use twig_core::keymap::{Action, Chord, Context, Key, Keymap, Modifiers};
use twig_core::repo::{self, FileDiff, Graph, RepoNode, StatusEntry};

use crate::keys::{self, KeyQueue};
use crate::session::{Session, SharedState};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Sidebar,
    Changes,
    RightTab,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Tab {
    Graph,
    Diff,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Sidebar,
    Changes,
    Main,
    Graph,
    Diff,
}

impl View {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "sidebar" => Self::Sidebar,
            "changes" => Self::Changes,
            "main" => Self::Main,
            "graph" => Self::Graph,
            "diff" => Self::Diff,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::Changes => "changes",
            Self::Main => "main",
            Self::Graph => "graph",
            Self::Diff => "diff",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    All,
    Single(View),
}

fn fixed_focus(view: View) -> Pane {
    match view {
        View::Sidebar => Pane::Sidebar,
        View::Changes => Pane::Changes,
        View::Main | View::Graph | View::Diff => Pane::RightTab,
    }
}

pub struct TuiApp {
    pub root: RepoNode,
    pub selected: PathBuf,
    pub staged: Vec<StatusEntry>,
    pub unstaged: Vec<StatusEntry>,
    pub focus: Pane,
    pub active_tab: Tab,
    pub view_mode: ViewMode,
    pub session: Option<Session>,
    pub quit_broadcast: bool,
    pub keymap: Keymap,
    pub pending_prefix: Option<Chord>,
    pub error: Option<String>,
    pub quit: bool,

    pub graph_limit: usize,

    pub sidebar_cursor: usize,
    pub changes_cursor: usize,
    pub changes_scroll: usize,
    pub changes_view_rows: usize,

    pub selected_file: Option<(String, bool)>,
    pub selected_commit: Option<Oid>,
    pub diff: FileDiff,
    pub diff_nav: DiffNavState,
    pub diff_scroll: usize,
    pub diff_center: bool,
    pub diff_hl: DiffHighlighter,
    pub diff_sig: u64,
    pub diff_view_rows: usize,

    pub graph: Graph,
    pub graph_cursor: usize,
    pub graph_scroll: usize,
    pub graph_view_rows: usize,

    pub commit_input: Option<String>,
    pub pending_editor: Option<PathBuf>,
    pub pending_copy: Option<String>,
    pub pending_focus_jump: bool,
}

pub struct SidebarRow {
    pub path: PathBuf,
    pub label: String,
    pub initialized: bool,
}

impl TuiApp {
    pub fn new(path: &Path) -> Result<Self, String> {
        Self::with_view(path, ViewMode::All)
    }

    pub fn with_view(path: &Path, view_mode: ViewMode) -> Result<Self, String> {
        let config = Config::load();
        let root = repo::discover(path).map_err(|e| e.to_string())?;
        let (staged, unstaged) = repo::load_status(path).map_err(|e| e.to_string())?;
        let graph_limit = config.graph_commit_limit;
        let graph = repo::build_graph(path, graph_limit).map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            selected: path.to_path_buf(),
            staged,
            unstaged,
            focus: match view_mode {
                ViewMode::All => Pane::Changes,
                ViewMode::Single(v) => fixed_focus(v),
            },
            active_tab: match view_mode {
                ViewMode::Single(View::Diff) => Tab::Diff,
                _ => Tab::Graph,
            },
            view_mode,
            session: None,
            quit_broadcast: true,
            keymap: Keymap::from_config(&config.keys),
            pending_prefix: None,
            error: None,
            quit: false,
            graph_limit,
            sidebar_cursor: 0,
            changes_cursor: 0,
            changes_scroll: 0,
            changes_view_rows: 20,
            selected_file: None,
            selected_commit: None,
            diff: FileDiff::empty(),
            diff_nav: DiffNavState::default(),
            diff_scroll: 0,
            diff_center: false,
            diff_hl: DiffHighlighter::default(),
            diff_sig: 0,
            diff_view_rows: 20,
            graph,
            graph_cursor: 0,
            graph_scroll: 0,
            graph_view_rows: 20,
            commit_input: None,
            pending_editor: None,
            pending_copy: None,
            pending_focus_jump: false,
        })
    }

    pub fn refresh(&mut self) {
        repo::refresh_badges(&mut self.root);
        match repo::load_status(&self.selected) {
            Ok((staged, unstaged)) => {
                self.staged = staged;
                self.unstaged = unstaged;
            }
            Err(e) => self.error = Some(format!("status failed: {e}")),
        }
        match repo::build_graph(&self.selected, self.graph_limit) {
            Ok(g) => {
                self.graph = g;
                self.graph_cursor = self.graph_cursor.min(self.graph_last());
            }
            Err(e) => self.error = Some(format!("graph failed: {e}")),
        }
        self.clamp_changes_cursor();
        if let Some((path, staged)) = self.selected_file.clone() {
            self.reload_file_diff(&path, staged);
        }
    }

    pub fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let mut out = Vec::new();
        push_sidebar_rows(&self.root, 0, &mut out);
        out
    }

    pub fn file_rows(&self) -> Vec<(String, bool)> {
        self.staged
            .iter()
            .map(|e| (e.path.clone(), true))
            .chain(self.unstaged.iter().map(|e| (e.path.clone(), false)))
            .collect()
    }

    fn clamp_changes_cursor(&mut self) {
        let n = self.staged.len() + self.unstaged.len();
        self.changes_cursor = self.changes_cursor.min(n.saturating_sub(1));
    }

    pub fn graph_last(&self) -> usize {
        self.graph.rows.len().saturating_sub(1)
    }

    fn select_repo(&mut self, path: PathBuf) {
        if self.selected == path {
            return;
        }
        self.selected = path;
        self.selected_file = None;
        self.selected_commit = None;
        self.diff = FileDiff::empty();
        self.diff_hl = DiffHighlighter::default();
        self.diff_sig = 0;
        self.diff_nav.reset();
        self.diff_scroll = 0;
        self.changes_cursor = 0;
        self.graph_cursor = 0;
        self.graph_scroll = 0;
        self.error = None;
        self.refresh();
    }

    fn open_file_diff(&mut self, path: String, staged: bool) {
        match repo::file_diff(&self.selected, &path, diff_mode(staged)) {
            Ok(d) => {
                self.diff = d;
                self.selected_file = Some((path.clone(), staged));
                self.selected_commit = None;
                self.rebuild_highlight(&path);
                self.diff_nav.reset();
                self.diff_nav.first_hunk(&self.diff.rows);
                self.diff_scroll = 0;
                self.diff_center = true;
                self.active_tab = Tab::Diff;
                self.focus = Pane::RightTab;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("diff failed: {e}")),
        }
    }

    fn reload_file_diff(&mut self, path: &str, staged: bool) {
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

    fn open_commit_diff(&mut self, oid: Oid) {
        match repo::commit_diff(&self.selected, oid) {
            Ok(d) => {
                self.diff = d;
                self.selected_commit = Some(oid);
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

    fn rebuild_highlight(&mut self, path: &str) {
        self.diff_sig = repo::hash_rows(&self.diff.rows);
        self.diff_hl = DiffHighlighter::new(path, &self.diff.rows, true);
    }

    pub fn handle_input(&mut self, events: Vec<KeyEvent>) {
        for ev in events {
            if self.quit {
                return;
            }
            if self.commit_input.is_some() {
                self.handle_commit_key(ev);
            } else if let Some(nk) = keys::normalize(&ev) {
                self.handle_key(nk);
            }
        }
    }

    fn handle_key(&mut self, nk: (Modifiers, Key)) {
        let mut queue = KeyQueue(vec![nk]);

        if queue.take(Modifiers::NONE, Key::Q) || queue.take(Modifiers::CTRL, Key::C) {
            self.quit = true;
            return;
        }
        if self.focus == Pane::Changes && queue.take(Modifiers::NONE, Key::C) {
            self.commit_input = Some(String::new());
            return;
        }

        match self.view_mode {
            ViewMode::All => self.handle_key_all(&mut queue),
            ViewMode::Single(view) => self.handle_key_single(view, &mut queue),
        }
    }

    fn handle_key_all(&mut self, queue: &mut KeyQueue) {
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
                )
            });
        for a in global {
            match a {
                Action::FocusLeft => self.focus_move(-1),
                Action::FocusRight => self.focus_move(1),
                Action::CycleTab | Action::CycleTabFwd => self.cycle_tab(1),
                Action::CycleTabBack => self.cycle_tab(-1),
                _ => {}
            }
        }

        match self.focus {
            Pane::Sidebar => self.sidebar_keys(queue),
            Pane::Changes => self.changes_keys(queue),
            Pane::RightTab => match self.active_tab {
                Tab::Graph => self.graph_keys(queue),
                Tab::Diff => self.diff_keys(queue),
            },
        }
    }

    fn handle_key_single(&mut self, view: View, queue: &mut KeyQueue) {
        let before = self.selection_snapshot();

        if view == View::Main {
            let global = self
                .keymap
                .resolve(queue, Context::Global, &mut self.pending_prefix, |a| {
                    matches!(
                        a,
                        Action::CycleTab | Action::CycleTabFwd | Action::CycleTabBack
                    )
                });
            for a in global {
                match a {
                    Action::CycleTab | Action::CycleTabFwd => self.cycle_tab(1),
                    Action::CycleTabBack => self.cycle_tab(-1),
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
            },
        }

        self.focus = fixed_focus(view);
        if self.selection_snapshot() != before {
            self.publish();
        }
    }

    fn selection_snapshot(&self) -> (PathBuf, Option<(String, bool)>, Option<Oid>, Tab) {
        (
            self.selected.clone(),
            self.selected_file.clone(),
            self.selected_commit,
            self.active_tab,
        )
    }

    fn publish(&mut self) {
        let repo = self.selected.clone();
        let file = self.selected_file.clone();
        let commit = self.selected_commit.map(|o| o.to_string());
        let tab = self.active_tab;
        if let Some(sess) = self.session.as_mut() {
            sess.publish(|st| {
                st.selected_repo = repo;
                st.selected_file = file;
                st.selected_commit = commit;
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
                if st.selected_commit != cur_commit
                    && let Ok(oid) = Oid::from_str(hex)
                {
                    self.open_commit_diff(oid);
                }
            } else if let Some((path, staged)) = &st.selected_file {
                if st.selected_file != self.selected_file {
                    self.open_file_diff(path.clone(), *staged);
                }
            } else if self.selected_file.is_some() || self.selected_commit.is_some() {
                self.selected_file = None;
                self.selected_commit = None;
                self.diff = FileDiff::empty();
                self.diff_nav.reset();
                self.diff_scroll = 0;
            }
        }
        if self.view_mode == ViewMode::Single(View::Main) {
            self.active_tab = st.active_tab;
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

    fn handle_commit_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        let Some(input) = self.commit_input.as_mut() else {
            return;
        };
        match ev.code {
            KeyCode::Esc => self.commit_input = None,
            KeyCode::Enter => self.run_commit(),
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) => {
                if ev.modifiers.contains(KeyModifiers::CONTROL) {
                    if c == 'c' {
                        self.commit_input = None;
                    }
                } else {
                    input.push(c);
                }
            }
            _ => {}
        }
    }

    fn run_commit(&mut self) {
        let msg = self.commit_input.take().unwrap_or_default();
        let msg = msg.trim();
        if msg.is_empty() {
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

    fn auto_stage_pointer(&mut self) {
        if let Some((parent_path, name)) = repo::find_submodule_parent(&self.root, &self.selected)
            && let Err(e) = repo::stage_submodule_pointer(&parent_path, &name)
        {
            self.error = Some(format!("stage submodule pointer failed: {e}"));
        }
    }

    fn sidebar_keys(&mut self, queue: &mut KeyQueue) {
        let rows = self.sidebar_rows();
        if rows.is_empty() {
            return;
        }
        let last = rows.len() - 1;
        let actions = self
            .keymap
            .resolve(queue, Context::Sidebar, &mut self.pending_prefix, |_| true);
        for a in actions {
            match a {
                Action::SidebarDown => self.sidebar_cursor = (self.sidebar_cursor + 1).min(last),
                Action::SidebarUp => self.sidebar_cursor = self.sidebar_cursor.saturating_sub(1),
                Action::SidebarTop => self.sidebar_cursor = 0,
                Action::SidebarBottom => self.sidebar_cursor = last,
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

    fn changes_keys(&mut self, queue: &mut KeyQueue) {
        let files = self.file_rows();
        let last = files.len().saturating_sub(1);
        let half = (self.changes_view_rows / 2).max(1);
        let actions = self
            .keymap
            .resolve(queue, Context::Changes, &mut self.pending_prefix, |_| true);
        for a in actions {
            match a {
                Action::ChangesDown => self.changes_cursor = (self.changes_cursor + 1).min(last),
                Action::ChangesUp => self.changes_cursor = self.changes_cursor.saturating_sub(1),
                Action::ChangesTop => self.changes_cursor = 0,
                Action::ChangesBottom => self.changes_cursor = last,
                Action::ChangesHalfPageDown => {
                    self.changes_cursor = (self.changes_cursor + half).min(last)
                }
                Action::ChangesHalfPageUp => {
                    self.changes_cursor = self.changes_cursor.saturating_sub(half)
                }
                Action::ChangesActivate | Action::ChangesExpand => {
                    if let Some((path, staged)) = files.get(self.changes_cursor).cloned() {
                        self.open_file_diff(path, staged);
                        self.pending_focus_jump = self.error.is_none();
                    }
                }
                Action::ChangesStageToggle => {
                    if let Some((path, staged)) = files.get(self.changes_cursor).cloned() {
                        self.toggle_stage(&path, staged);
                    }
                }
                Action::ChangesEdit => {
                    if let Some((path, _)) = files.get(self.changes_cursor) {
                        self.pending_editor = Some(self.selected.join(path));
                    }
                }
                _ => {}
            }
        }
    }

    fn toggle_stage(&mut self, path: &str, staged: bool) {
        let paths = vec![path.to_string()];
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

    fn diff_keys(&mut self, queue: &mut KeyQueue) {
        let actions = self
            .keymap
            .resolve(queue, Context::Diff, &mut self.pending_prefix, |a| {
                !matches!(
                    a,
                    Action::DiffFind
                        | Action::DiffStageSelection
                        | Action::DiffUnstageSelection
                        | Action::DiffDiscardSelection
                )
            });
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
                Action::DiffClearVisual => self.diff_nav.anchor = None,
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
                        self.pending_editor = Some(self.selected.join(path));
                    }
                }
                _ => {}
            }
        }
    }

    fn graph_keys(&mut self, queue: &mut KeyQueue) {
        let last = self.graph_last();
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
                )
            });
        for a in actions {
            match a {
                Action::GraphDown => self.graph_cursor = (self.graph_cursor + 1).min(last),
                Action::GraphUp => self.graph_cursor = self.graph_cursor.saturating_sub(1),
                Action::GraphTop => self.graph_cursor = 0,
                Action::GraphBottom => self.graph_cursor = last,
                Action::GraphHalfPageDown => {
                    self.graph_cursor = (self.graph_cursor + half).min(last)
                }
                Action::GraphHalfPageUp => {
                    self.graph_cursor = self.graph_cursor.saturating_sub(half)
                }
                Action::GraphOpen => {
                    if let Some(row) = self.graph.rows.get(self.graph_cursor)
                        && !row.is_uncommitted
                    {
                        self.open_commit_diff(row.id);
                        self.pending_focus_jump = self.error.is_none();
                    }
                }
                _ => {}
            }
        }
    }

    fn focus_move(&mut self, dir: isize) {
        let order = [Pane::Sidebar, Pane::Changes, Pane::RightTab];
        let cur = order.iter().position(|p| *p == self.focus).unwrap_or(1) as isize;
        let next = (cur + dir).clamp(0, order.len() as isize - 1) as usize;
        self.focus = order[next];
    }

    fn cycle_tab(&mut self, dir: isize) {
        if self.focus != Pane::RightTab {
            self.focus = Pane::RightTab;
            return;
        }
        let order = [Tab::Graph, Tab::Diff];
        let cur = order.iter().position(|t| *t == self.active_tab).unwrap_or(0) as isize;
        let next = (cur + dir).rem_euclid(order.len() as isize) as usize;
        self.active_tab = order[next];
    }
}

fn diff_mode(staged: bool) -> repo::DiffMode {
    if staged {
        repo::DiffMode::Staged
    } else {
        repo::DiffMode::Unstaged
    }
}

fn push_sidebar_rows(node: &RepoNode, depth: usize, out: &mut Vec<SidebarRow>) {
    let mut label = format!("{}{}", "  ".repeat(depth), node.name);
    if !node.initialized {
        label.push_str(" (uninit)");
    }
    if node.drifted {
        label.push_str(" *drift");
    }
    if node.dirty {
        label.push_str(" *dirty");
    }
    out.push(SidebarRow {
        path: node.path.clone(),
        label,
        initialized: node.initialized,
    });
    for child in &node.children {
        push_sidebar_rows(child, depth + 1, out);
    }
}
