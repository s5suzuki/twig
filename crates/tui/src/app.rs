use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};
use twig_core::config::Config;
use twig_core::diffnav::DiffNavState;
use twig_core::git2::Oid;
use twig_core::highlight::DiffHighlighter;
use twig_core::keymap::{Action, Chord, Context, Key, Keymap, Modifiers};
use twig_core::repo::{self, CommitFile, FileDiff, Graph, RepoNode, StatusEntry};

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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Prompt {
    Commit,
    Amend,
    ConfirmAmendPushed,
    ConfirmDiscardFiles { paths: Vec<String>, label: String },
    ConfirmDiscardLines { path: String, lo: usize, hi: usize },
}

impl Prompt {
    pub fn wants_text(&self) -> bool {
        matches!(self, Prompt::Commit | Prompt::Amend)
    }

    pub fn label(&self) -> String {
        match self {
            Prompt::Commit => "Commit message:".to_string(),
            Prompt::Amend => "Amend message:".to_string(),
            Prompt::ConfirmAmendPushed => {
                "HEAD is already pushed. Amend anyway? (y/n)".to_string()
            }
            Prompt::ConfirmDiscardFiles { label, .. } => {
                format!("Discard changes to {label}? (y/n)")
            }
            Prompt::ConfirmDiscardLines { path, .. } => {
                format!("Discard selected lines in {path}? (y/n)")
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GraphItem {
    Commit(usize),
    File(usize),
}

type Snapshot = (
    PathBuf,
    Option<(String, bool)>,
    Option<Oid>,
    Option<String>,
    Tab,
);

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
    pub selected_commit_file: Option<String>,
    pub commit_files: Vec<CommitFile>,
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

    pub prompt: Option<(Prompt, String)>,
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
            selected_commit_file: None,
            commit_files: Vec::new(),
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
            prompt: None,
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
        if let Some(oid) = self.selected_commit
            && !self.graph.rows.iter().any(|r| r.id == oid)
        {
            self.selected_commit = None;
            self.selected_commit_file = None;
            self.commit_files.clear();
            self.graph_cursor = self.graph_cursor.min(self.graph_last());
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

    pub fn graph_items(&self) -> Vec<GraphItem> {
        let mut out = Vec::with_capacity(self.graph.rows.len() + self.commit_files.len());
        for (i, row) in self.graph.rows.iter().enumerate() {
            out.push(GraphItem::Commit(i));
            if self.selected_commit == Some(row.id) {
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

    fn select_repo(&mut self, path: PathBuf) {
        if self.selected == path {
            return;
        }
        self.selected = path;
        self.selected_file = None;
        self.selected_commit = None;
        self.selected_commit_file = None;
        self.commit_files.clear();
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
                self.selected_commit_file = None;
                self.commit_files.clear();
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
                self.selected_commit_file = None;
                self.commit_files = repo::commit_files(&self.selected, oid).unwrap_or_default();
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

    fn open_commit_file_diff(&mut self, oid: Oid, path: String) {
        match repo::commit_file_diff(&self.selected, oid, &path) {
            Ok(d) => {
                self.diff = d;
                self.selected_commit = Some(oid);
                self.selected_commit_file = Some(path.clone());
                if self.commit_files.is_empty() {
                    self.commit_files =
                        repo::commit_files(&self.selected, oid).unwrap_or_default();
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

    fn collapse_commit(&mut self) {
        self.selected_commit = None;
        self.selected_commit_file = None;
        self.commit_files.clear();
        self.diff = FileDiff::empty();
        self.diff_hl = DiffHighlighter::default();
        self.diff_sig = 0;
        self.diff_nav.reset();
        self.diff_scroll = 0;
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
            if self.prompt.is_some() {
                self.handle_prompt_key(ev);
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
            self.prompt = Some((Prompt::Commit, String::new()));
            return;
        }
        if self.focus == Pane::Changes && queue.take(Modifiers::NONE, Key::A) {
            self.open_amend_prompt();
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

    fn selection_snapshot(&self) -> Snapshot {
        (
            self.selected.clone(),
            self.selected_file.clone(),
            self.selected_commit,
            self.selected_commit_file.clone(),
            self.active_tab,
        )
    }

    fn publish(&mut self) {
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

    fn handle_prompt_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        let Some((kind, input)) = self.prompt.as_mut() else {
            return;
        };
        if kind.wants_text() {
            match ev.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Enter => self.submit_prompt(),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => {
                    if ev.modifiers.contains(KeyModifiers::CONTROL) {
                        if c == 'c' {
                            self.prompt = None;
                        }
                    } else {
                        input.push(c);
                    }
                }
                _ => {}
            }
        } else {
            match ev.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.submit_prompt(),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.prompt = None,
                KeyCode::Char(c)
                    if c == 'c' && ev.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.prompt = None
                }
                _ => {}
            }
        }
    }

    fn submit_prompt(&mut self) {
        let Some((kind, input)) = self.prompt.take() else {
            return;
        };
        match kind {
            Prompt::Commit => self.run_commit(&input),
            Prompt::Amend => {
                if input.trim().is_empty() {
                    return;
                }
                if repo::head_is_pushed(&self.selected) {
                    self.prompt = Some((Prompt::ConfirmAmendPushed, input));
                } else {
                    self.run_amend(&input);
                }
            }
            Prompt::ConfirmAmendPushed => self.run_amend(&input),
            Prompt::ConfirmDiscardFiles { paths, .. } => self.run_discard_files(&paths),
            Prompt::ConfirmDiscardLines { path, lo, hi } => self.run_discard_lines(&path, lo, hi),
        }
    }

    fn run_commit(&mut self, msg: &str) {
        let msg = msg.trim();
        if msg.is_empty() {
            self.prompt = Some((Prompt::Commit, String::new()));
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

    fn open_amend_prompt(&mut self) {
        if repo::seq_state(&self.selected) != repo::SeqState::None {
            self.error = Some("finish or abort the in-progress operation first".to_string());
            return;
        }
        if !repo::head_has_commit(&self.selected) {
            self.error = Some("nothing to amend".to_string());
            return;
        }
        let msg = repo::head_message(&self.selected)
            .map(|m| m.trim_end().to_string())
            .unwrap_or_default();
        self.prompt = Some((Prompt::Amend, msg));
    }

    fn run_amend(&mut self, msg: &str) {
        match repo::amend(&self.selected, Some(msg.trim())) {
            Ok(_) => {
                self.auto_stage_pointer();
                self.refresh();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("amend failed: {e}")),
        }
    }

    fn run_discard_files(&mut self, paths: &[String]) {
        match repo::discard(&self.selected, paths) {
            Ok(()) => {
                self.refresh();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("discard failed: {e}")),
        }
    }

    fn run_discard_lines(&mut self, path: &str, lo: usize, hi: usize) {
        if let Err(e) = repo::discard_partial(&self.selected, path, &self.diff.rows, lo, hi) {
            self.error = Some(format!("discard failed: {e}"));
        } else {
            self.error = None;
        }
        self.diff_nav.anchor = None;
        self.refresh();
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
                Action::ChangesDiscard => {
                    if let Some((path, false)) = files.get(self.changes_cursor).cloned() {
                        let mut paths = vec![path.clone()];
                        if let Some(old) = self
                            .unstaged
                            .iter()
                            .find(|e| e.path == path)
                            .and_then(|e| e.old_path.clone())
                        {
                            paths.push(old);
                        }
                        self.prompt = Some((
                            Prompt::ConfirmDiscardFiles { paths, label: path },
                            String::new(),
                        ));
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
                !matches!(a, Action::DiffFind)
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
                Action::DiffStageSelection => self.apply_line_selection(false),
                Action::DiffUnstageSelection => self.apply_line_selection(true),
                Action::DiffDiscardSelection => self.request_discard_selection(),
                Action::DiffStageHunk => self.apply_hunk_at_cursor(false),
                Action::DiffUnstageHunk => self.apply_hunk_at_cursor(true),
                _ => {}
            }
        }
    }

    fn worktree_diff_target(&self, want_staged: bool) -> Option<String> {
        if self.diff.conflict || self.diff.rename {
            return None;
        }
        match &self.selected_file {
            Some((path, staged)) if *staged == want_staged => Some(path.clone()),
            _ => None,
        }
    }

    fn apply_line_selection(&mut self, unstage: bool) {
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

    fn request_discard_selection(&mut self) {
        let Some(path) = self.worktree_diff_target(false) else {
            return;
        };
        let Some((lo, hi)) = self.diff_nav.action_range(&self.diff.rows) else {
            return;
        };
        self.prompt = Some((Prompt::ConfirmDiscardLines { path, lo, hi }, String::new()));
    }

    fn apply_hunk_at_cursor(&mut self, unstage: bool) {
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

    fn hunk_range_at_cursor(&self) -> Option<(usize, usize)> {
        let rows = &self.diff.rows;
        if rows.is_empty() {
            return None;
        }
        let cursor = self.diff_nav.cursor.min(rows.len() - 1);
        let is_boundary =
            |r: &repo::DiffRow| matches!(r, repo::DiffRow::Hunk { .. } | repo::DiffRow::FileHeader(_));
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

    fn graph_keys(&mut self, queue: &mut KeyQueue) {
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
                )
            });
        for a in actions {
            let last = self.graph_last();
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
                Action::GraphOpen => self.graph_open(),
                Action::GraphCollapse => self.graph_collapse(),
                _ => {}
            }
        }
    }

    fn graph_open(&mut self) {
        let items = self.graph_items();
        match items.get(self.graph_cursor.min(items.len().saturating_sub(1))) {
            Some(GraphItem::Commit(row)) => {
                let row = &self.graph.rows[*row];
                if row.is_uncommitted {
                    return;
                }
                if self.selected_commit == Some(row.id) && self.selected_commit_file.is_none() {
                    self.collapse_commit();
                    return;
                }
                let oid = row.id;
                self.open_commit_diff(oid);
                if self.error.is_none() {
                    self.set_graph_cursor_to_commit(oid);
                    self.pending_focus_jump = true;
                }
            }
            Some(GraphItem::File(k)) => {
                if let (Some(oid), Some(f)) = (self.selected_commit, self.commit_files.get(*k)) {
                    let path = f.path.clone();
                    self.open_commit_file_diff(oid, path);
                    self.pending_focus_jump = self.error.is_none();
                }
            }
            None => {}
        }
    }

    fn graph_collapse(&mut self) {
        let items = self.graph_items();
        let cursor = self.graph_cursor.min(items.len().saturating_sub(1));
        match items.get(cursor) {
            Some(GraphItem::File(_)) => {
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

    fn set_graph_cursor_to_commit(&mut self, oid: Oid) {
        if let Some(idx) = self.graph_items().iter().position(
            |it| matches!(it, GraphItem::Commit(r) if self.graph.rows[*r].id == oid),
        ) {
            self.graph_cursor = idx;
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
