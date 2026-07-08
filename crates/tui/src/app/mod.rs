use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};
use twit_core::config::Config;
use twit_core::diffnav::DiffNavState;
use twit_core::git2::Oid;
use twit_core::highlight::DiffHighlighter;
use twit_core::keymap::{Action, Chord, Context, Key, Keymap, Modifiers};
use twit_core::repo::{self, CommitFile, FileDiff, Graph, RepoNode, StatusEntry};

use crate::keys::{self, KeyQueue};
use crate::session::{Session, SharedState};

mod changes;
mod diff;
mod editor;
mod graph;
mod graph_ops;
mod input;
mod nav;
mod prompt;
mod remote;
mod search;
mod session_sync;
pub use changes::*;
pub use diff::*;
use editor::*;
pub use graph::*;
pub use graph_ops::*;
use nav::*;
pub use prompt::*;
pub use remote::*;
pub use search::*;

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
    Search,
    Editor,
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

const NAV_HISTORY_MAX: usize = 100;

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

    pub config: Config,

    pub sidebar_cursor: usize,
    pub sidebar_view_rows: usize,
    pub changes_cursor: usize,
    pub changes_scroll: usize,
    pub changes_view_rows: usize,
    pub changes_folds: HashSet<(bool, String)>,

    pub selected_file: Option<(String, bool)>,
    pub selected_commit: Option<Oid>,
    pub selected_commit_file: Option<String>,
    pub commit_files: Vec<CommitFile>,
    pub commit_folds: HashSet<String>,
    pub commit_detail: Vec<String>,
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
    pub pending_editor: Option<(PathBuf, Option<u32>)>,
    pub pending_shell: Option<Vec<String>>,
    pub stashes: Vec<repo::StashEntry>,
    pub seq: Option<(repo::SeqState, Vec<String>)>,
    pub remote: Option<RemoteJob>,
    pub diff_find: Option<String>,
    pub search: SearchState,
    pub help_open: bool,
    pub help_scroll: usize,
    pub settings_open: bool,
    pub settings_cursor: usize,
    diff_recheck: u8,
    diff_recheck_at: Option<std::time::Instant>,
    pub pending_copy: Option<String>,
    pub pending_focus_jump: bool,
    pub term: Option<crate::term::EditorTerm>,
    pub nvim_socket: PathBuf,
    pub pending_open: Option<(PathBuf, Option<u32>, std::time::Instant)>,
    pub editor_area: Option<ratatui::layout::Rect>,
    pub editor_cursor_shape: Option<crate::term::CursorStyle>,
    mouse_pressed: Option<u8>,
    last_mouse_cell: Option<(u16, u16)>,
    editor_seq_seen: Option<u64>,

    nav_back: Vec<NavPoint>,
    nav_fwd: Vec<NavPoint>,
    nav_current: Option<NavPoint>,
}

pub struct SidebarRow {
    pub path: PathBuf,
    pub label: String,
    pub name: String,
    pub parent: Option<PathBuf>,
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
        let graph =
            repo::build_graph(path, config.graph_commit_limit).map_err(|e| e.to_string())?;
        let mut app = Self {
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
                ViewMode::Single(View::Main) => Tab::Editor,
                _ => Tab::Graph,
            },
            view_mode,
            session: None,
            quit_broadcast: false,
            keymap: Keymap::from_config(&config.keys),
            pending_prefix: None,
            error: None,
            quit: false,
            config,
            sidebar_cursor: 0,
            sidebar_view_rows: 20,
            changes_cursor: 0,
            changes_scroll: 0,
            changes_view_rows: 20,
            changes_folds: HashSet::new(),
            selected_file: None,
            selected_commit: None,
            selected_commit_file: None,
            commit_files: Vec::new(),
            commit_folds: HashSet::new(),
            commit_detail: Vec::new(),
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
            pending_shell: None,
            stashes: Vec::new(),
            seq: None,
            remote: None,
            diff_find: None,
            search: SearchState::default(),
            help_open: false,
            help_scroll: 0,
            settings_open: false,
            settings_cursor: 0,
            diff_recheck: 0,
            diff_recheck_at: None,
            pending_copy: None,
            pending_focus_jump: false,
            term: None,
            nvim_socket: nvim_socket_path(),
            pending_open: None,
            editor_area: None,
            editor_cursor_shape: None,
            mouse_pressed: None,
            last_mouse_cell: None,
            editor_seq_seen: None,
            nav_back: Vec::new(),
            nav_fwd: Vec::new(),
            nav_current: None,
        };
        app.sync_stash_and_seq();
        app.nav_current = Some(app.current_nav_point());
        Ok(app)
    }

    fn sync_stash_and_seq(&mut self) {
        self.stashes = repo::stash_list(&self.selected);
        self.seq = match repo::seq_state(&self.selected) {
            repo::SeqState::None => None,
            kind => Some((kind, repo::seq_conflicts(&self.selected))),
        };
    }

    pub fn refresh(&mut self) {
        repo::refresh_badges(&mut self.root);
        self.sync_stash_and_seq();
        match repo::load_status(&self.selected) {
            Ok((staged, unstaged)) => {
                self.staged = staged;
                self.unstaged = unstaged;
            }
            Err(e) => self.error = Some(format!("status failed: {e}")),
        }
        match repo::build_graph(&self.selected, self.config.graph_commit_limit) {
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
            self.commit_folds.clear();
            self.commit_detail.clear();
            self.graph_cursor = self.graph_cursor.min(self.graph_last());
        }
        self.clamp_changes_cursor();
        if let Some((path, staged)) = self.selected_file.clone() {
            self.reload_file_diff(&path, staged);
            self.arm_diff_recheck();
        }
    }

    pub fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let mut out = Vec::new();
        push_sidebar_rows(&self.root, 0, None, &mut out);
        out
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
        self.commit_folds.clear();
        self.commit_detail.clear();
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
}

fn push_sidebar_rows(
    node: &RepoNode,
    depth: usize,
    parent: Option<&Path>,
    out: &mut Vec<SidebarRow>,
) {
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
        name: node.name.clone(),
        parent: parent.map(Path::to_path_buf),
        initialized: node.initialized,
    });
    for child in &node.children {
        push_sidebar_rows(child, depth + 1, Some(&node.path), out);
    }
}
