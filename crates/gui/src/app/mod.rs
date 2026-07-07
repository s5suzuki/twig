use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use git2::Oid;

use crate::keys::{Chord, Keymap};
use twit_core::config::Config;
use twit_core::diffnav::{self, DiffNavState};
use twit_core::repo::{self, DiffMode, DiffRow, FileDiff, Graph, RepoNode, StatusEntry};
use twit_core::search;

mod diff;
mod editor;
mod find;
mod graph;
mod graph_ops;
mod nav;
mod ops;
mod remote;
mod search_tab;
pub use diff::*;
use editor::*;
pub use find::*;
pub use graph::*;
pub use graph_ops::*;
use nav::*;
pub use remote::*;
pub use search_tab::*;

pub const LIST_PAGE: usize = 10;
const NAV_HISTORY_MAX: usize = 100;
const DIFF_RECHECK_TRIES: u8 = 5;
const DIFF_RECHECK_INTERVAL: f64 = 0.1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Graph,
    Diff,
    Editor,
    Search,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sidebar,
    Changes,
    RightTab,
    Terminal,
}

#[derive(Clone, Copy)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

pub struct App {
    pub root: Option<RepoNode>,
    pub error: Option<String>,

    pub selected: PathBuf,
    pub graph: Graph,
    pub staged: Vec<StatusEntry>,
    pub unstaged: Vec<StatusEntry>,

    pub selected_file: Option<(String, bool)>,
    pub selected_commit: Option<(Oid, String)>,
    pub commit_files: Vec<repo::CommitFile>,
    pub commit_detail: String,
    pub selected_commit_file: Option<String>,
    pub commit_folds: HashSet<String>,
    pub diff: FileDiff,
    pub diff_hl: twit_core::highlight::DiffHighlighter,
    pub diff_galleys: crate::ui::diff_view::DiffGalleyCache,
    diff_ver: u64,
    diff_sig: u64,
    diff_recheck: u8,
    diff_recheck_at: f64,
    in_recheck: bool,
    diff_hl_sig: Option<(u64, bool)>,
    pub diff_nav: DiffNavState,
    pub diff_scroll_pending: bool,
    pub diff_scroll_center: bool,
    pub diff_scrolled_prev: bool,
    pub diff_visible: Option<(usize, usize)>,
    pub commit_msg: String,
    pub amend_mode: bool,
    saved_commit_msg: Option<String>,
    pub confirm_amend: bool,
    pub active_tab: Tab,

    pub find: FindBar,
    pub search: SearchState,
    pub search_confirm: bool,

    pub term: Option<crate::term::Term>,
    pub nvim_socket: PathBuf,

    pub pending_open: Option<(PathBuf, Option<u32>)>,

    pub shell: Option<crate::term::Term>,
    pub shell_open: bool,
    pending_shell_cmd: Option<String>,

    pub focus: Pane,
    pub changes_cursor: usize,
    pub changes_scroll_pending: bool,
    pub sidebar_cursor: usize,
    pub sidebar_scroll_pending: bool,
    pub file_cache: HashMap<PathBuf, Vec<repo::FileNode>>,
    pub graph_cursor: usize,
    pub graph_scroll_pending: bool,
    pub graph_menu: Option<GraphMenu>,
    pub keymap: Keymap,
    pub pending_prefix: Option<Chord>,
    pub confirm_discard: Option<DiscardReq>,
    pub confirm_discard_range: Option<(String, usize, usize)>,

    pub seq: Option<SeqStatus>,
    pub stashes: Vec<repo::StashEntry>,
    pub ref_prompt: Option<RefPrompt>,
    pub name_input: String,
    pub name_input_focus: bool,
    pub confirm_delete: Option<DeleteTarget>,
    pub reset_prompt: Option<Oid>,
    pub confirm_op: Option<(GraphOp, Oid)>,
    pub confirm_force_push: bool,

    pub config: Config,
    pub settings_open: bool,
    pub help_open: bool,

    pub remote_busy: bool,
    pub remote_kind: RemoteKind,
    pub remote_progress: Option<(usize, usize)>,
    remote_task: Option<Receiver<RemoteMsg>>,

    watch_root: PathBuf,
    watcher: Option<twit_core::watch::WorktreeWatcher>,
    watcher_started: bool,

    repaint_gate: Arc<AtomicBool>,
    was_hidden: bool,

    nav_back: Vec<NavPoint>,
    nav_fwd: Vec<NavPoint>,
    nav_current: Option<NavPoint>,
}

impl App {
    pub fn new(path: PathBuf) -> Self {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let config = Config::load();
        let keymap = Keymap::from_config(&config.keys);
        let mut app = Self {
            root: None,
            error: None,
            selected: path.clone(),
            graph: Graph {
                rows: Vec::new(),
                max_col: 0,
            },
            staged: Vec::new(),
            unstaged: Vec::new(),
            selected_file: None,
            selected_commit: None,
            commit_files: Vec::new(),
            commit_detail: String::new(),
            selected_commit_file: None,
            commit_folds: HashSet::new(),
            diff: empty_diff(),
            diff_hl: twit_core::highlight::DiffHighlighter::default(),
            diff_galleys: crate::ui::diff_view::DiffGalleyCache::default(),
            diff_ver: 0,
            diff_sig: 0,
            diff_recheck: 0,
            diff_recheck_at: 0.0,
            in_recheck: false,
            diff_hl_sig: None,
            diff_nav: DiffNavState::default(),
            diff_scroll_pending: false,
            diff_scroll_center: false,
            diff_scrolled_prev: false,
            diff_visible: None,
            commit_msg: String::new(),
            amend_mode: false,
            saved_commit_msg: None,
            confirm_amend: false,
            active_tab: Tab::Graph,
            find: FindBar::default(),
            search: SearchState::default(),
            search_confirm: false,
            term: None,
            nvim_socket: std::env::temp_dir()
                .join(format!("twig-nvim-{}.sock", std::process::id())),
            pending_open: None,
            shell: None,
            shell_open: true,
            pending_shell_cmd: None,
            focus: Pane::Changes,
            changes_cursor: 0,
            changes_scroll_pending: false,
            sidebar_cursor: 0,
            sidebar_scroll_pending: false,
            file_cache: HashMap::new(),
            graph_cursor: 0,
            graph_scroll_pending: false,
            graph_menu: None,
            keymap,
            pending_prefix: None,
            confirm_discard: None,
            confirm_discard_range: None,
            seq: None,
            stashes: Vec::new(),
            ref_prompt: None,
            name_input: String::new(),
            name_input_focus: false,
            confirm_delete: None,
            reset_prompt: None,
            confirm_op: None,
            confirm_force_push: false,
            config,
            settings_open: false,
            help_open: false,
            remote_busy: false,
            remote_kind: RemoteKind::Fetch,
            remote_progress: None,
            remote_task: None,
            watch_root: path.clone(),
            watcher: None,
            watcher_started: false,
            repaint_gate: Arc::new(AtomicBool::new(true)),
            was_hidden: false,
            nav_back: Vec::new(),
            nav_fwd: Vec::new(),
            nav_current: None,
        };

        match repo::discover(&path) {
            Ok(node) => app.root = Some(node),
            Err(e) => app.error = Some(format!("Cannot open repository: {e}")),
        }
        app.select_repo(path);
        app
    }

    pub fn apply_config(&self, ctx: &egui::Context) {
        ctx.set_zoom_factor(self.config.font_size / twit_core::config::BASE_FONT_SIZE);
        ctx.set_visuals(crate::theme::visuals(&self.config));
    }

    pub fn select_repo(&mut self, path: PathBuf) {
        self.selected = path.clone();
        self.reset_amend_mode();
        self.selected_file = None;
        self.clear_commit_selection();
        self.diff = empty_diff();
        self.nav_back.clear();
        self.nav_fwd.clear();
        self.nav_current = None;
        self.graph_cursor = 0;
        self.graph_menu = None;
        self.reload();
    }

    pub fn ensure_watcher(&mut self, ctx: &egui::Context) {
        if self.watcher_started {
            return;
        }
        self.watcher_started = true;
        let ctx = ctx.clone();
        let gate = self.repaint_gate();
        let notifier: twit_core::watch::Notifier = Arc::new(move || {
            if gate.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
        });
        match twit_core::watch::WorktreeWatcher::new(&self.watch_root, notifier) {
            Ok(w) => self.watcher = Some(w),
            Err(e) => self.error = Some(e),
        }
    }

    pub fn take_external_change(&mut self) -> bool {
        self.watcher.as_ref().is_some_and(|w| w.take_dirty())
    }

    pub fn update_visibility(&mut self, ctx: &egui::Context) {
        let (occluded, minimized, focused) = ctx.input(|i| {
            let vp = i.viewport();
            (vp.occluded, vp.minimized, vp.focused)
        });
        let hidden = focused == Some(false) || occluded == Some(true) || minimized == Some(true);

        self.repaint_gate.store(!hidden, Ordering::Relaxed);

        if self.was_hidden && !hidden {
            ctx.request_repaint();
        }
        self.was_hidden = hidden;
    }

    pub fn repaint_gate(&self) -> Arc<AtomicBool> {
        self.repaint_gate.clone()
    }

    pub fn refresh_from_disk(&mut self) {
        if let Some(w) = self.watcher.as_mut() {
            w.rescan_new_toplevel(&self.watch_root);
        }
        self.file_cache.clear();
        self.after_index_change();
    }

    pub fn reload(&mut self) {
        match repo::build_graph(&self.selected, self.config.graph_commit_limit) {
            Ok(graph) => {
                self.graph = graph;
                self.error = None;
            }
            Err(e) => {
                self.graph = Graph {
                    rows: Vec::new(),
                    max_col: 0,
                };
                self.error = Some(format!("Failed to build graph: {e}"));
            }
        }
        match repo::load_status(&self.selected) {
            Ok((staged, unstaged)) => {
                self.staged = staged;
                self.unstaged = unstaged;
            }
            Err(e) => {
                self.staged.clear();
                self.unstaged.clear();
                self.error = Some(format!("Failed to read status: {e}"));
            }
        }
        if self
            .selected_commit
            .as_ref()
            .is_some_and(|(o, _)| o.is_zero())
        {
            let files = self.uncommitted_commit_files();
            if files.is_empty() {
                self.clear_commit_selection();
                self.diff = empty_diff();
                self.diff_ver = self.diff_ver.wrapping_add(1);
            } else {
                if let Some(p) = &self.selected_commit_file
                    && !files.iter().any(|f| &f.path == p)
                {
                    self.selected_commit_file = None;
                }
                self.commit_files = files;
            }
        }
        self.sync_seq_state();
        self.stashes = repo::stash_list(&self.selected);
        if let Some(root) = &mut self.root {
            repo::refresh_badges(root);
        }
    }

    pub fn any_modal_open(&self) -> bool {
        self.settings_open
            || self.confirm_discard.is_some()
            || self.confirm_discard_range.is_some()
            || self.ref_prompt.is_some()
            || self.confirm_delete.is_some()
            || self.reset_prompt.is_some()
            || self.confirm_op.is_some()
            || self.confirm_force_push
            || self.confirm_amend
            || self.search_confirm
    }

    pub fn help_context(&self) -> Option<crate::keys::Context> {
        use crate::keys::Context;
        match self.focus {
            Pane::Sidebar => Some(Context::Sidebar),
            Pane::Changes => Some(Context::Changes),
            Pane::RightTab => match self.active_tab {
                Tab::Graph => Some(Context::Graph),
                Tab::Diff if self.selected_file.is_some() => Some(Context::Diff),
                _ => None,
            },
            Pane::Terminal => None,
        }
    }

    pub fn move_focus(&mut self, dir: Dir) {
        use Dir::*;
        use Pane::*;
        let shell = self.shell_open;
        self.focus = match (self.focus, dir) {
            (Sidebar, Right) => Changes,
            (Changes, Left) => Sidebar,
            (Changes, Right) => RightTab,
            (Changes, Down) if shell => Terminal,
            (RightTab, Left) => Changes,
            (RightTab, Down) if shell => Terminal,
            (Terminal, Up) => RightTab,
            (Terminal, Left) => Changes,
            (cur, _) => cur,
        };
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::ui::draw(self, ui);
    }
}
