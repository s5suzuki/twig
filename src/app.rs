use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use git2::Oid;

use crate::config::Config;
use crate::keys::{Chord, Keymap};
use crate::repo::{self, DiffMode, DiffRow, FileDiff, Graph, RepoNode, StatusEntry};
use crate::search;

pub const LIST_PAGE: usize = 10;
const NAV_HISTORY_MAX: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Graph,
    Diff,
    Editor,
    Search,
}

pub struct FindMatch {
    pub row: usize,
    pub line_no: u32,
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct FindBar {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub error: Option<String>,
    pub focus_request: bool,
    pub matches: Vec<FindMatch>,
    pub current: usize,
    sig: Option<(String, bool, bool)>,
}

impl FindBar {
    pub fn invalidate(&mut self) {
        self.sig = None;
    }

    pub fn recompute(&mut self, diff: &FileDiff) {
        let sig = (self.query.clone(), self.regex, self.case_sensitive);
        if self.sig.as_ref() == Some(&sig) {
            return;
        }
        self.sig = Some(sig);
        self.matches.clear();
        self.error = None;
        if self.query.is_empty() {
            self.current = 0;
            return;
        }
        let matcher = match search::Matcher::new(&self.query, self.regex, self.case_sensitive) {
            Ok(m) => m,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        for (row, r) in diff.rows.iter().enumerate() {
            if let DiffRow::Line {
                right: Some(text),
                new_no,
                ..
            } = r
            {
                for (start, end) in search::line_ranges(&matcher, text) {
                    self.matches.push(FindMatch {
                        row,
                        line_no: new_no.unwrap_or(0),
                        start,
                        end,
                    });
                }
            }
        }
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub replace: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub error: Option<String>,
    pub results: Vec<search::FileHit>,
    pub selected: HashSet<(String, u32)>,
    pub searched: bool,
    pub focus_request: bool,
}

impl SearchState {
    pub fn selected_count(&self) -> usize {
        self.results
            .iter()
            .flat_map(|f| f.lines.iter().map(move |l| (f.path.clone(), l.line_no)))
            .filter(|k| self.selected.contains(k))
            .count()
    }
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeqKind {
    Rebase,
    RebaseInteractive,
    CherryPick,
    Revert,
    Merge,
}

impl SeqKind {
    pub fn label(self) -> &'static str {
        match self {
            SeqKind::Rebase => "Rebase",
            SeqKind::RebaseInteractive => "Interactive rebase",
            SeqKind::CherryPick => "Cherry-pick",
            SeqKind::Revert => "Revert",
            SeqKind::Merge => "Merge",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemoteKind {
    Fetch,
    Pull,
    Push,
    DeleteRemote,
    SubmoduleInit,
    SubmoduleUpdate,
}

impl RemoteKind {
    fn verb(self) -> &'static str {
        match self {
            RemoteKind::Fetch => "Fetch",
            RemoteKind::Pull => "Pull",
            RemoteKind::Push => "Push",
            RemoteKind::DeleteRemote => "Delete remote branch",
            RemoteKind::SubmoduleInit => "Initialize submodule",
            RemoteKind::SubmoduleUpdate => "Update submodule",
        }
    }

    fn rediscovers(self) -> bool {
        matches!(self, RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate)
    }
}

enum RemoteMsg {
    Progress { received: usize, total: usize },
    Done(Result<repo::SeqOutcome, String>),
}

pub struct SeqStatus {
    pub kind: SeqKind,
    pub conflicts: Vec<String>,
}

pub enum RefPrompt {
    CreateBranch { at: Oid },
    RenameBranch { from: String },
    CreateTag { at: Oid },
}

pub enum DeleteTarget {
    Branch(String),
    Tag(String),
    RemoteBranch(String),
}

#[derive(Clone, Copy)]
pub enum GraphItem {
    Commit(usize),
    File(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GraphOp {
    CherryPick,
    Revert,
    RebaseOnto,
    Checkout,
}

impl GraphOp {
    pub fn title(self) -> &'static str {
        match self {
            GraphOp::CherryPick => "Cherry-pick commit",
            GraphOp::Revert => "Revert commit",
            GraphOp::RebaseOnto => "Rebase onto commit",
            GraphOp::Checkout => "Check out commit",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            GraphOp::CherryPick => "Apply this commit's changes onto the current branch as a new commit.",
            GraphOp::Revert => "Create a new commit on the current branch that undoes this commit's changes.",
            GraphOp::RebaseOnto => "Replay the current branch onto this commit. This rewrites the branch's commits.",
            GraphOp::Checkout => "Check out this commit directly (detached HEAD).",
        }
    }
}

pub struct GraphMenu {
    pub oid: Oid,
    pub pos: egui::Pos2,
    pub cursor: usize,
}

#[derive(Clone, PartialEq)]
enum NavSel {
    None,
    File { path: String, staged: bool },
    Commit { oid: Oid },
    CommitFile { oid: Oid, path: String },
}

#[derive(Clone)]
struct NavPoint {
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
    pub diff: FileDiff,
    pub diff_hl: crate::highlight::DiffHighlight,
    pub diff_galleys: crate::ui::diff_view::DiffGalleyCache,
    diff_ver: u64,
    diff_sig: u64,
    diff_hl_sig: Option<(u64, bool)>,
    pub diff_cursor: usize,
    pub diff_anchor: Option<usize>,
    pub diff_scroll_pending: bool,
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

    pub pending_open: Option<PathBuf>,

    pub shell: Option<crate::term::Term>,
    pub shell_open: bool,
    pending_shell_cmd: Option<String>,

    pub focus: Pane,
    pub changes_cursor: usize,
    pub sidebar_cursor: usize,
    pub graph_cursor: usize,
    pub graph_scroll_pending: bool,
    pub graph_menu: Option<GraphMenu>,
    pub keymap: Keymap,
    pub pending_prefix: Option<Chord>,
    pub confirm_discard: Option<String>,

    pub seq: Option<SeqStatus>,
    pub stashes: Vec<repo::StashEntry>,
    pub ref_prompt: Option<RefPrompt>,
    pub name_input: String,
    pub name_input_focus: bool,
    pub confirm_delete: Option<DeleteTarget>,
    pub reset_prompt: Option<Oid>,
    pub confirm_op: Option<(GraphOp, Oid)>,

    pub config: Config,
    pub settings_open: bool,
    pub help_open: bool,

    pub remote_busy: bool,
    pub remote_kind: RemoteKind,
    pub remote_progress: Option<(usize, usize)>,
    remote_task: Option<Receiver<RemoteMsg>>,

    watch_root: PathBuf,
    watcher: Option<crate::watch::WorktreeWatcher>,
    watcher_started: bool,

    repaint_gate: Arc<AtomicBool>,
    was_hidden: bool,

    nav_back: Vec<NavPoint>,
    nav_fwd: Vec<NavPoint>,
    nav_current: Option<NavPoint>,
}

impl App {
    pub fn new(path: PathBuf) -> Self {
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
            diff: empty_diff(),
            diff_hl: crate::highlight::DiffHighlight::default(),
            diff_galleys: crate::ui::diff_view::DiffGalleyCache::default(),
            diff_ver: 0,
            diff_sig: 0,
            diff_hl_sig: None,
            diff_cursor: 0,
            diff_anchor: None,
            diff_scroll_pending: false,
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
            sidebar_cursor: 0,
            graph_cursor: 0,
            graph_scroll_pending: false,
            graph_menu: None,
            keymap,
            pending_prefix: None,
            confirm_discard: None,
            seq: None,
            stashes: Vec::new(),
            ref_prompt: None,
            name_input: String::new(),
            name_input_focus: false,
            confirm_delete: None,
            reset_prompt: None,
            confirm_op: None,
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
        ctx.set_zoom_factor(self.config.font_size / crate::config::BASE_FONT_SIZE);
        ctx.set_visuals(self.config.visuals());
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

    fn clear_commit_selection(&mut self) {
        self.selected_commit = None;
        self.commit_files.clear();
        self.commit_detail.clear();
        self.selected_commit_file = None;
    }

    pub fn ensure_watcher(&mut self, ctx: &egui::Context) {
        if self.watcher_started {
            return;
        }
        self.watcher_started = true;
        match crate::watch::WorktreeWatcher::new(&self.watch_root, ctx, self.repaint_gate()) {
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
        let hidden =
            focused == Some(false) || occluded == Some(true) || minimized == Some(true);

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
        if self.selected_commit.as_ref().is_some_and(|(o, _)| o.is_zero()) {
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

    fn load_file_diff(&mut self, file: String, staged: bool) {
        let mode = if staged {
            DiffMode::Staged
        } else {
            DiffMode::Unstaged
        };
        let prev = self.selected_file.clone();
        match repo::file_diff(&self.selected, &file, mode) {
            Ok(d) => {
                let sig = hash_diff(&d.rows);
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
                };
                self.diff_sig = 0;
                self.diff_ver = self.diff_ver.wrapping_add(1);
                self.find.invalidate();
            }
        }
        self.selected_file = Some((file, staged));
        self.clear_commit_selection();

        self.clamp_diff_nav();
    }

    pub fn diff_version(&self) -> u64 {
        self.diff_ver
    }

    fn diff_path(&self) -> Option<String> {
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
                crate::highlight::highlight_diff(&path, &self.diff.rows, dark)
            }
            _ => crate::highlight::DiffHighlight::default(),
        };
    }

    pub fn open_find(&mut self) {
        if self.selected_file.is_none() {
            return;
        }
        self.active_tab = Tab::Diff;
        self.focus = Pane::RightTab;
        self.find.open = true;
        self.find.focus_request = true;
    }

    pub fn close_find(&mut self) {
        self.find.open = false;
    }

    pub fn toggle_find(&mut self) {
        if self.find.open {
            self.close_find();
        } else {
            self.open_find();
        }
    }

    fn scroll_to_find(&mut self) {
        if let Some(m) = self.find.matches.get(self.find.current) {
            self.diff_cursor = m.row.min(self.diff_last_row());
            self.diff_scroll_pending = true;
        }
    }

    pub fn find_next(&mut self) {
        if self.find.matches.is_empty() {
            return;
        }
        self.find.current = (self.find.current + 1) % self.find.matches.len();
        self.scroll_to_find();
    }

    pub fn find_prev(&mut self) {
        if self.find.matches.is_empty() {
            return;
        }
        let n = self.find.matches.len();
        self.find.current = (self.find.current + n - 1) % n;
        self.scroll_to_find();
    }

    fn find_matcher(&mut self) -> Option<search::Matcher> {
        match search::Matcher::new(&self.find.query, self.find.regex, self.find.case_sensitive) {
            Ok(m) => Some(m),
            Err(e) => {
                self.find.error = Some(e);
                None
            }
        }
    }

    pub fn find_replace_current(&mut self) {
        let Some((path, staged)) = self.selected_file.clone() else {
            return;
        };
        if staged {
            return;
        }
        let Some(m) = self
            .find
            .matches
            .get(self.find.current)
            .map(|m| (m.line_no, m.start))
        else {
            return;
        };
        let Some(matcher) = self.find_matcher() else {
            return;
        };
        let replacement = self.find.replace.clone();
        let abs = self.selected.join(&path);
        let Ok(text) = std::fs::read_to_string(&abs) else {
            return;
        };
        if let Some(new) = search::replace_one_in_text(&matcher, &text, m.0, m.1, &replacement)
            && std::fs::write(&abs, new).is_ok()
        {
            self.find.invalidate();
            self.after_index_change();
        }
    }

    pub fn find_replace_all(&mut self) {
        let Some((path, staged)) = self.selected_file.clone() else {
            return;
        };
        if staged {
            return;
        }
        let Some(matcher) = self.find_matcher() else {
            return;
        };
        let replacement = self.find.replace.clone();
        let abs = self.selected.join(&path);
        let Ok(text) = std::fs::read_to_string(&abs) else {
            return;
        };
        let (new, n) = search::replace_all_in_text(&matcher, &text, &replacement);
        if n > 0 && new != text && std::fs::write(&abs, new).is_ok() {
            self.find.invalidate();
            self.after_index_change();
        }
    }

    pub fn search_run(&mut self) {
        self.search.error = None;
        self.search.results.clear();
        self.search.selected.clear();
        self.search.searched = true;
        let matcher =
            match search::Matcher::new(&self.search.query, self.search.regex, self.search.case_sensitive)
            {
                Ok(m) => m,
                Err(e) => {
                    self.search.error = Some(e);
                    return;
                }
            };
        let hits = search::search_repo(&self.selected, &matcher);
        for f in &hits {
            for l in &f.lines {
                self.search.selected.insert((f.path.clone(), l.line_no));
            }
        }
        self.search.results = hits;
    }

    pub fn search_apply(&mut self) {
        self.search_confirm = false;
        let matcher =
            match search::Matcher::new(&self.search.query, self.search.regex, self.search.case_sensitive)
            {
                Ok(m) => m,
                Err(e) => {
                    self.search.error = Some(e);
                    return;
                }
            };
        let replacement = self.search.replace.clone();
        let mut errs = Vec::new();
        for f in &self.search.results {
            let lines: Vec<u32> = f
                .lines
                .iter()
                .map(|l| l.line_no)
                .filter(|ln| self.search.selected.contains(&(f.path.clone(), *ln)))
                .collect();
            if lines.is_empty() {
                continue;
            }
            let abs = self.selected.join(&f.path);
            let Ok(mut text) = std::fs::read_to_string(&abs) else {
                continue;
            };
            for ln in lines {
                if let Some(new) = search::replace_line_in_text(&matcher, &text, ln, &replacement) {
                    text = new;
                }
            }
            if let Err(e) = std::fs::write(&abs, text) {
                errs.push(format!("{}: {e}", f.path));
            }
        }
        if !errs.is_empty() {
            self.error = Some(errs.join("; "));
        }
        self.search_run();
        self.reload();
    }

    pub fn search_toggle_line(&mut self, path: &str, line_no: u32) {
        let key = (path.to_string(), line_no);
        if !self.search.selected.remove(&key) {
            self.search.selected.insert(key);
        }
    }

    pub fn search_file_all_selected(&self, f: &search::FileHit) -> bool {
        f.lines
            .iter()
            .all(|l| self.search.selected.contains(&(f.path.clone(), l.line_no)))
    }

    pub fn search_toggle_file(&mut self, idx: usize) {
        let Some(f) = self.search.results.get(idx) else {
            return;
        };
        let all = self.search_file_all_selected(f);
        let keys: Vec<(String, u32)> =
            f.lines.iter().map(|l| (f.path.clone(), l.line_no)).collect();
        for k in keys {
            if all {
                self.search.selected.remove(&k);
            } else {
                self.search.selected.insert(k);
            }
        }
    }

    pub fn search_select_all(&mut self, select: bool) {
        self.search.selected.clear();
        if select {
            for f in &self.search.results {
                for l in &f.lines {
                    self.search.selected.insert((f.path.clone(), l.line_no));
                }
            }
        }
    }

    fn reset_diff_nav(&mut self) {
        self.diff_cursor = 0;
        self.diff_anchor = None;
        self.diff_scroll_pending = false;
        self.diff_visible = None;
    }

    fn clamp_diff_nav(&mut self) {
        let last = self.diff_last_row();
        if self.diff_cursor > last {
            self.diff_cursor = last;
        }
        if let Some(a) = self.diff_anchor
            && a > last {
                self.diff_anchor = Some(last);
            }

        if !self.diff.rows.is_empty() && !self.is_line_row(self.diff_cursor) {
            let fwd = (self.diff_cursor..=last).find(|&i| self.is_line_row(i));
            let back = (0..self.diff_cursor).rev().find(|&i| self.is_line_row(i));
            if let Some(i) = fwd.or(back) {
                self.diff_cursor = i;
            }
        }
    }

    pub fn diff_last_row(&self) -> usize {
        self.diff.rows.len().saturating_sub(1)
    }

    pub fn move_diff_cursor(&mut self, delta: isize) {
        let last = self.diff_last_row();
        let cur = self.diff_cursor.min(last);

        if !self.diff_scrolled_prev
            && let Some((vt, vb)) = self.diff_visible {
                if cur < vt {
                    self.diff_cursor = vt;
                    self.diff_scroll_pending = true;
                    return;
                }
                if cur > vb {
                    self.diff_cursor = vb;
                    self.diff_scroll_pending = true;
                    return;
                }
            }
        self.diff_cursor = self.step_line_row(cur, delta);
        self.diff_scroll_pending = true;
    }

    fn is_line_row(&self, i: usize) -> bool {
        matches!(self.diff.rows.get(i), Some(DiffRow::Line { .. }))
    }

    fn step_line_row(&self, from: usize, delta: isize) -> usize {
        let last = self.diff_last_row();
        let step = if delta >= 0 { 1 } else { -1 };
        let mut i = from as isize;
        loop {
            let ni = i + step;
            if ni < 0 || ni > last as isize {
                return from;
            }
            i = ni;
            if self.is_line_row(i as usize) {
                return i as usize;
            }
        }
    }

    pub fn set_diff_cursor(&mut self, row: usize) {
        self.diff_cursor = row.min(self.diff_last_row());
        self.diff_scroll_pending = true;
    }

    pub fn toggle_diff_visual(&mut self) {
        self.diff_anchor = if self.diff_anchor.is_some() {
            None
        } else {
            Some(self.diff_cursor)
        };
    }

    pub fn diff_highlight(&self) -> Option<(usize, usize)> {
        self.diff_anchor.map(|a| {
            let last = self.diff_last_row();
            let a = a.min(last);
            let c = self.diff_cursor.min(last);
            (a.min(c), a.max(c))
        })
    }

    fn diff_action_range(&self) -> Option<(usize, usize)> {
        if self.diff.rows.is_empty() {
            return None;
        }
        let last = self.diff_last_row();
        let c = self.diff_cursor.min(last);
        Some(match self.diff_anchor {
            Some(a) => {
                let a = a.min(last);
                (a.min(c), a.max(c))
            }
            None => (c, c),
        })
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
        self.diff_anchor = None;
        self.after_index_change();
    }

    pub fn select_file(&mut self, file: String, staged: bool) {
        self.reset_diff_nav();
        self.load_file_diff(file, staged);
        self.active_tab = Tab::Diff;
        self.focus = Pane::RightTab;
    }

    pub fn select_commit(&mut self, oid: Oid) {
        if self
            .selected_commit
            .as_ref()
            .is_some_and(|(o, _)| *o == oid)
        {
            self.clear_commit_selection();
            self.diff = empty_diff();
            self.diff_ver = self.diff_ver.wrapping_add(1);
            return;
        }
        self.load_commit(oid);
    }

    fn load_commit(&mut self, oid: Oid) {
        if oid.is_zero() {
            self.commit_files = self.uncommitted_commit_files();
            self.commit_detail = String::new();
            self.selected_commit = Some((oid, "Uncommitted Changes".to_string()));
        } else {
            let short = oid.to_string();
            let label = short[..7.min(short.len())].to_string();
            self.commit_files = repo::commit_files(&self.selected, oid).unwrap_or_default();
            self.commit_detail = repo::commit_message(&self.selected, oid).unwrap_or_default();
            self.selected_commit = Some((oid, label));
        }
        self.selected_file = None;
        self.selected_commit_file = None;
        self.diff = FileDiff {
            rows: Vec::new(),
            note: Some("Select a file".to_string()),
            conflict: false,
            rename: false,
        };
        self.diff_ver = self.diff_ver.wrapping_add(1);
    }

    fn uncommitted_commit_files(&self) -> Vec<repo::CommitFile> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for e in self.unstaged.iter().chain(self.staged.iter()) {
            if seen.insert(e.path.clone()) {
                out.push(repo::CommitFile {
                    path: e.path.clone(),
                    kind: e.kind,
                });
            }
        }
        out
    }

    pub fn select_commit_file(&mut self, file: String) {
        let Some((oid, _)) = self.selected_commit else {
            return;
        };
        let result = if oid.is_zero() {
            let staged = self.worktree_file_staged(&file);
            let mode = if staged {
                DiffMode::Staged
            } else {
                DiffMode::Unstaged
            };
            repo::file_diff(&self.selected, &file, mode)
        } else {
            repo::commit_file_diff(&self.selected, oid, &file)
        };
        match result {
            Ok(d) => {
                self.diff_sig = hash_diff(&d.rows);
                self.diff = d;
            }
            Err(e) => {
                self.diff_sig = 0;
                self.diff = FileDiff {
                    rows: Vec::new(),
                    note: Some(format!("file diff failed: {e}")),
                    conflict: false,
                    rename: false,
                }
            }
        }
        self.selected_file = None;
        self.selected_commit_file = Some(file);
        self.diff_ver = self.diff_ver.wrapping_add(1);
        self.reset_diff_nav();
        self.active_tab = Tab::Diff;
    }

    fn worktree_file_staged(&self, file: &str) -> bool {
        !self.unstaged.iter().any(|e| e.path == file)
            && self.staged.iter().any(|e| e.path == file)
    }

    pub fn graph_items(&self) -> Vec<GraphItem> {
        let sel = self.selected_commit.as_ref().map(|(o, _)| *o);
        let mut out = Vec::with_capacity(self.graph.rows.len());
        for (i, row) in self.graph.rows.iter().enumerate() {
            out.push(GraphItem::Commit(i));
            if Some(row.id) == sel {
                for k in 0..self.commit_files.len() {
                    out.push(GraphItem::File(k));
                }
            }
        }
        out
    }

    pub fn clamp_graph_cursor(&mut self) {
        let n = self.graph_items().len();
        if n == 0 {
            self.graph_cursor = 0;
        } else if self.graph_cursor >= n {
            self.graph_cursor = n - 1;
        }
    }

    fn parent_commit_item(&self, items: &[GraphItem], from: usize) -> Option<usize> {
        (0..=from.min(items.len().saturating_sub(1)))
            .rev()
            .find(|&i| matches!(items[i], GraphItem::Commit(_)))
    }

    pub fn move_graph_cursor(&mut self, delta: isize) {
        let n = self.graph_items().len();
        if n == 0 {
            return;
        }
        let last = (n - 1) as isize;
        let cur = self.graph_cursor.min(n - 1) as isize;
        self.graph_cursor = (cur + delta).clamp(0, last) as usize;
        self.graph_scroll_pending = true;
    }

    pub fn set_graph_cursor(&mut self, idx: usize) {
        let n = self.graph_items().len();
        if n == 0 {
            return;
        }
        self.graph_cursor = idx.min(n - 1);
        self.graph_scroll_pending = true;
    }

    pub fn graph_cursor_bottom(&mut self) {
        let n = self.graph_items().len();
        if n > 0 {
            self.graph_cursor = n - 1;
            self.graph_scroll_pending = true;
        }
    }

    pub fn set_graph_cursor_to_commit(&mut self, oid: Oid) {
        if let Some(idx) = self
            .graph_items()
            .iter()
            .position(|it| matches!(it, GraphItem::Commit(row) if self.graph.rows[*row].id == oid))
        {
            self.graph_cursor = idx;
        }
    }

    pub fn graph_target_commit(&self) -> Option<Oid> {
        let items = self.graph_items();
        let idx = self.graph_cursor.min(items.len().checked_sub(1)?);
        let oid = match items.get(idx)? {
            GraphItem::Commit(r) => Some(self.graph.rows[*r].id),
            GraphItem::File(_) => match items[self.parent_commit_item(&items, idx)?] {
                GraphItem::Commit(r) => Some(self.graph.rows[r].id),
                GraphItem::File(_) => None,
            },
        };
        oid.filter(|o| !o.is_zero())
    }

    pub fn set_graph_cursor_to_file(&mut self, path: &str) {
        if let Some(idx) = self.graph_items().iter().position(|it| {
            matches!(it, GraphItem::File(k) if self.commit_files.get(*k).is_some_and(|f| f.path == path))
        }) {
            self.graph_cursor = idx;
            self.graph_scroll_pending = true;
        }
    }

    pub fn graph_activate(&mut self) {
        let items = self.graph_items();
        let Some(&item) = items.get(self.graph_cursor) else {
            return;
        };
        match item {
            GraphItem::Commit(row) => {
                let oid = self.graph.rows[row].id;
                let already = self.selected_commit.as_ref().is_some_and(|(o, _)| *o == oid);
                if !already {
                    self.load_commit(oid);
                    self.set_graph_cursor_to_commit(oid);
                }
            }
            GraphItem::File(k) => {
                if let Some(f) = self.commit_files.get(k) {
                    let path = f.path.clone();
                    self.select_commit_file(path);
                }
            }
        }
        self.graph_scroll_pending = true;
    }

    pub fn graph_open_editor(&mut self) {
        let items = self.graph_items();
        if let Some(GraphItem::File(k)) = items.get(self.graph_cursor).copied()
            && let Some(f) = self.commit_files.get(k)
        {
            let path = f.path.clone();
            self.open_in_editor(&path);
        }
    }

    pub fn graph_collapse(&mut self) {
        let items = self.graph_items();
        let Some(&item) = items.get(self.graph_cursor) else {
            return;
        };
        match item {
            GraphItem::File(_) => {
                if let Some(ci) = self.parent_commit_item(&items, self.graph_cursor) {
                    self.graph_cursor = ci;
                    self.graph_scroll_pending = true;
                }
            }
            GraphItem::Commit(row) => {
                let oid = self.graph.rows[row].id;
                if self.selected_commit.as_ref().is_some_and(|(o, _)| *o == oid) {
                    self.clear_commit_selection();
                    self.diff = empty_diff();
                    self.diff_ver = self.diff_ver.wrapping_add(1);
                }
            }
        }
    }

    fn current_nav_point(&self) -> NavPoint {
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
            diff_cursor: self.diff_cursor,
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

    fn restore_nav(&mut self, p: NavPoint) {
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
            self.diff_cursor = p.diff_cursor.min(self.diff_last_row());
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

    pub fn open_in_editor(&mut self, file: &str) {
        let abs = self.selected.join(file);
        self.active_tab = Tab::Editor;
        self.focus = Pane::RightTab;
        if self.term.is_some() && self.nvim_socket.exists() {
            if let Err(e) = crate::editor::open_abs_in_server(&abs, &self.nvim_socket) {
                self.error = Some(e);
            }
        } else {
            self.pending_open = Some(abs);
        }
    }

    pub fn flush_pending_open(&mut self) -> bool {
        let Some(abs) = self.pending_open.clone() else {
            return false;
        };
        if self.term.is_none() || !self.nvim_socket.exists() {
            return true;
        }
        if let Err(e) = crate::editor::open_abs_in_server(&abs, &self.nvim_socket) {
            self.error = Some(e);
        }
        self.pending_open = None;
        false
    }

    pub fn toggle_shell(&mut self) {
        self.shell_open = !self.shell_open;
    }

    pub fn terminal_focused(&self) -> bool {
        self.focus == Pane::Terminal
            || (self.focus == Pane::RightTab && matches!(self.active_tab, Tab::Editor))
    }

    pub fn any_modal_open(&self) -> bool {
        self.settings_open
            || self.confirm_discard.is_some()
            || self.ref_prompt.is_some()
            || self.confirm_delete.is_some()
            || self.reset_prompt.is_some()
            || self.confirm_op.is_some()
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

    pub fn scroll_diff(&mut self, fraction: f32, down: bool) {
        let visible_rows = match self.diff_visible {
            Some((top, bottom)) => bottom.saturating_sub(top) + 1,
            None => 20,
        };
        let steps = ((visible_rows as f32 * fraction).round() as isize).max(1);
        let mut cur = self.diff_cursor.min(self.diff_last_row());
        let dir = if down { 1 } else { -1 };
        for _ in 0..steps {
            let next = self.step_line_row(cur, dir);
            if next == cur {
                break;
            }
            cur = next;
        }
        self.diff_cursor = cur;
        self.diff_scroll_pending = true;
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

    pub fn ensure_shell(&mut self, ctx: &egui::Context) {
        if self.shell.as_mut().is_some_and(|t| !t.is_alive()) {
            self.shell = None;
        }
        if self.shell.is_none() {
            match crate::term::Term::spawn_shell(&self.watch_root, ctx, self.repaint_gate()) {
                Ok(t) => self.shell = Some(t),
                Err(e) => self.error = Some(e),
            }
        }
    }

    pub fn flush_pending_shell_cmd(&mut self) {
        let Some(cmd) = self.pending_shell_cmd.take() else {
            return;
        };
        if let Some(sh) = &mut self.shell {
            let mut line = cmd.into_bytes();
            line.push(b'\n');
            sh.feed(&line);
        }
    }

    pub fn interactive_rebase(&mut self, oid: Oid) {
        if self.busy_with_seq() {
            return;
        }
        let base = match repo::commit_parent_count(&self.selected, oid) {
            Ok(0) => "--root".to_string(),
            Ok(_) => format!("{oid}^"),
            Err(e) => {
                self.error = Some(format!("rebase failed: {e}"));
                return;
            }
        };
        let dir = sh_quote(&self.selected.to_string_lossy());
        self.pending_shell_cmd = Some(format!("git -C {dir} rebase -i {base}"));
        self.shell_open = true;
        self.focus = Pane::Terminal;
    }

    pub fn discard_changes(&mut self, path: &str) {
        let paths = self.entry_paths(path);
        if let Err(e) = repo::discard(&self.selected, &paths) {
            self.error = Some(format!("Failed to discard changes: {e}"));
        }
        self.after_index_change();
    }

    fn entry_paths(&self, path: &str) -> Vec<String> {
        self.staged
            .iter()
            .chain(self.unstaged.iter())
            .find(|e| e.path == path)
            .map(|e| e.paths())
            .unwrap_or_else(|| vec![path.to_string()])
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

    fn auto_stage_pointer(&mut self) {
        let parent = self
            .root
            .as_ref()
            .and_then(|root| find_submodule_parent(root, &self.selected));
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

    fn reset_amend_mode(&mut self) {
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

    fn busy_with_seq(&mut self) -> bool {
        if self.seq.is_some() {
            self.error = Some("Finish or abort the in-progress operation first".to_string());
            true
        } else {
            false
        }
    }

    pub fn switch_branch(&mut self, name: String) {
        if self.busy_with_seq() {
            return;
        }
        match repo::checkout_branch(&self.selected, &name) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("switch failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn checkout_commit(&mut self, oid: Oid) {
        if self.busy_with_seq() {
            return;
        }
        match repo::checkout_commit(&self.selected, oid) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("checkout failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn rebase_onto(&mut self, onto: Oid) {
        let r = repo::rebase_onto(&self.selected, onto);
        self.apply_seq_outcome(SeqKind::Rebase, r, "rebase");
    }

    pub fn cherry_pick(&mut self, oid: Oid) {
        let r = repo::cherry_pick(&self.selected, oid);
        self.apply_seq_outcome(SeqKind::CherryPick, r, "cherry-pick");
    }

    pub fn revert(&mut self, oid: Oid) {
        let r = repo::revert(&self.selected, oid);
        self.apply_seq_outcome(SeqKind::Revert, r, "revert");
    }

    pub fn seq_continue(&mut self) {
        let Some(kind) = self.seq.as_ref().map(|s| s.kind) else {
            return;
        };
        let r = match kind {
            SeqKind::Rebase => repo::rebase_continue(&self.selected),
            SeqKind::RebaseInteractive => return,
            SeqKind::CherryPick => repo::cherry_pick_continue(&self.selected),
            SeqKind::Revert => repo::revert_continue(&self.selected),
            SeqKind::Merge => repo::merge_continue(&self.selected),
        };
        self.apply_seq_outcome(kind, r, "continue");
    }

    pub fn seq_abort(&mut self) {
        let Some(kind) = self.seq.as_ref().map(|s| s.kind) else {
            return;
        };
        let r = match kind {
            SeqKind::Rebase => repo::rebase_abort(&self.selected),
            SeqKind::RebaseInteractive => return,
            SeqKind::CherryPick => repo::cherry_pick_abort(&self.selected),
            SeqKind::Revert => repo::revert_abort(&self.selected),
            SeqKind::Merge => repo::merge_abort(&self.selected),
        };
        if let Err(e) = r {
            self.error = Some(format!("{} --abort failed: {e}", kind.label()));
        }
        self.seq = None;
        self.after_commit_topology_change();
    }

    fn apply_seq_outcome(
        &mut self,
        kind: SeqKind,
        r: Result<repo::SeqOutcome, git2::Error>,
        what: &str,
    ) {
        match r {
            Ok(repo::SeqOutcome::Done) => {
                self.seq = None;
                self.error = None;
            }
            Ok(repo::SeqOutcome::Conflicts(files)) => {
                self.seq = Some(SeqStatus {
                    kind,
                    conflicts: files,
                });
            }
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn begin_create_branch(&mut self, at: Oid) {
        if self.busy_with_seq() {
            return;
        }
        self.name_input = String::new();
        self.name_input_focus = true;
        self.ref_prompt = Some(RefPrompt::CreateBranch { at });
    }

    pub fn begin_rename_branch(&mut self, from: String) {
        self.name_input = from.clone();
        self.name_input_focus = true;
        self.ref_prompt = Some(RefPrompt::RenameBranch { from });
    }

    pub fn begin_create_tag(&mut self, at: Oid) {
        self.name_input = String::new();
        self.name_input_focus = true;
        self.ref_prompt = Some(RefPrompt::CreateTag { at });
    }

    pub fn commit_ref_prompt(&mut self, switch: bool) {
        let name = self.name_input.trim().to_string();
        let Some(prompt) = self.ref_prompt.take() else {
            return;
        };
        if name.is_empty() {
            self.error = Some("Name is empty".to_string());
            return;
        }
        let res = match &prompt {
            RefPrompt::CreateBranch { at } => repo::create_branch(&self.selected, &name, *at)
                .and_then(|()| {
                    if switch {
                        repo::checkout_branch(&self.selected, &name)
                    } else {
                        Ok(())
                    }
                }),
            RefPrompt::RenameBranch { from } => repo::rename_branch(&self.selected, from, &name),
            RefPrompt::CreateTag { at } => repo::create_tag(&self.selected, &name, *at),
        };
        match res {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("ref op failed: {e}")),
        }

        self.after_commit_topology_change();
    }

    pub fn delete_ref(&mut self, target: &DeleteTarget) {
        let res = match target {
            DeleteTarget::Branch(name) => repo::delete_branch(&self.selected, name),
            DeleteTarget::Tag(name) => repo::delete_tag(&self.selected, name),
            DeleteTarget::RemoteBranch(_) => return,
        };
        if let Err(e) = res {
            self.error = Some(format!("delete failed: {e}"));
        }
        self.reload();
    }

    pub fn run_confirmed_op(&mut self) {
        let Some((op, oid)) = self.confirm_op.take() else {
            return;
        };
        match op {
            GraphOp::CherryPick => self.cherry_pick(oid),
            GraphOp::Revert => self.revert(oid),
            GraphOp::RebaseOnto => self.rebase_onto(oid),
            GraphOp::Checkout => self.checkout_commit(oid),
        }
    }

    pub fn do_reset(&mut self, oid: Oid, mode: repo::ResetMode) {
        if self.busy_with_seq() {
            return;
        }
        match repo::reset(&self.selected, oid, mode) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("reset failed: {e}")),
        }
        self.after_commit_topology_change();
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

    fn run_stash(&mut self, r: Result<(), git2::Error>, what: &str) {
        match r {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn checkout_tracking(&mut self, remote_ref: String) {
        if self.busy_with_seq() {
            return;
        }
        match repo::checkout_tracking(&self.selected, &remote_ref) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("checkout failed: {e}")),
        }
        self.after_commit_topology_change();
    }

    pub fn fetch(&mut self, ctx: &egui::Context) {
        let remote = repo::primary_remote(&self.selected);
        self.start_remote(ctx, RemoteKind::Fetch, remote, Vec::new());
    }

    pub fn pull(&mut self, ctx: &egui::Context) {
        if self.busy_with_seq() {
            return;
        }
        let remote = repo::primary_remote(&self.selected);
        self.start_remote(ctx, RemoteKind::Pull, remote, Vec::new());
    }

    pub fn push(&mut self, ctx: &egui::Context, force: bool) {
        let remote = repo::primary_remote(&self.selected);
        let Some(mut refspec) = repo::head_push_refspec(&self.selected) else {
            self.error = Some("Not on a branch to push".to_string());
            return;
        };
        if force {
            refspec.insert(0, '+');
        }
        self.start_remote(ctx, RemoteKind::Push, remote, vec![refspec]);
    }

    pub fn delete_remote_branch(&mut self, ctx: &egui::Context, remote_ref: String) {
        let Some((remote, branch)) = remote_ref.split_once('/') else {
            self.error = Some(format!("Invalid remote branch: {remote_ref}"));
            return;
        };
        self.start_remote(
            ctx,
            RemoteKind::DeleteRemote,
            Some(remote.to_string()),
            vec![branch.to_string()],
        );
    }

    fn start_remote(
        &mut self,
        ctx: &egui::Context,
        kind: RemoteKind,
        remote: Option<String>,
        refspecs: Vec<String>,
    ) {
        if self.remote_busy {
            self.error = Some("A remote operation is already running".to_string());
            return;
        }
        let Some(remote) = remote else {
            self.error = Some("No remote configured".to_string());
            return;
        };

        let (tx, rx) = mpsc::channel();
        let path = self.selected.clone();
        let ctx = ctx.clone();
        let gate = self.repaint_gate.clone();

        self.remote_busy = true;
        self.remote_kind = kind;
        self.remote_progress = None;
        self.remote_task = Some(rx);
        self.error = None;

        thread::spawn(move || {
            let progress = |received, total| {
                let _ = tx.send(RemoteMsg::Progress { received, total });
                if gate.load(Ordering::Relaxed) {
                    ctx.request_repaint();
                }
            };
            let result = match kind {
                RemoteKind::Fetch => {
                    repo::fetch(&path, &remote, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::Push => {
                    repo::push(&path, &remote, &refspecs, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::Pull => repo::pull(&path, &remote, progress),
                RemoteKind::DeleteRemote => {
                    let branch = refspecs.first().cloned().unwrap_or_default();
                    repo::delete_remote_branch(&path, &remote, &branch, progress)
                        .map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate => {
                    Err(git2::Error::from_str("not a remote operation"))
                }
            };
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
            if gate.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
        });
    }

    pub fn submodule_init(&mut self, ctx: &egui::Context, parent: PathBuf, name: String) {
        self.start_submodule(ctx, RemoteKind::SubmoduleInit, parent, name);
    }

    pub fn submodule_update(&mut self, ctx: &egui::Context, parent: PathBuf, name: String) {
        self.start_submodule(ctx, RemoteKind::SubmoduleUpdate, parent, name);
    }

    fn start_submodule(
        &mut self,
        ctx: &egui::Context,
        kind: RemoteKind,
        parent: PathBuf,
        name: String,
    ) {
        if self.remote_busy {
            self.error = Some("A remote operation is already running".to_string());
            return;
        }

        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let gate = self.repaint_gate.clone();

        self.remote_busy = true;
        self.remote_kind = kind;
        self.remote_progress = None;
        self.remote_task = Some(rx);
        self.error = None;

        thread::spawn(move || {
            let progress = |received, total| {
                let _ = tx.send(RemoteMsg::Progress { received, total });
                if gate.load(Ordering::Relaxed) {
                    ctx.request_repaint();
                }
            };
            let result = match kind {
                RemoteKind::SubmoduleUpdate => repo::submodule_update(&parent, &name, progress),
                _ => repo::submodule_init(&parent, &name, progress),
            }
            .map(|()| repo::SeqOutcome::Done);
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
            if gate.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
        });
    }

    pub fn poll_remote(&mut self) {
        let Some(rx) = &self.remote_task else {
            return;
        };
        let mut done = None;
        loop {
            match rx.try_recv() {
                Ok(RemoteMsg::Progress { received, total }) => {
                    self.remote_progress = Some((received, total));
                }
                Ok(RemoteMsg::Done(res)) => {
                    done = Some(res);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    done = Some(Err("remote worker exited unexpectedly".to_string()));
                    break;
                }
            }
        }
        if let Some(res) = done {
            self.finish_remote(res);
        }
    }

    fn finish_remote(&mut self, res: Result<repo::SeqOutcome, String>) {
        self.remote_task = None;
        self.remote_busy = false;
        self.remote_progress = None;
        let rediscover = self.remote_kind.rediscovers();
        if rediscover && res.is_ok() {
            self.rediscover();
        }
        self.after_commit_topology_change();
        match res {
            Ok(_) => self.error = None,
            Err(e) => self.error = Some(format!("{} failed: {e}", self.remote_kind.verb())),
        }
    }

    fn rediscover(&mut self) {
        let mut expanded = Vec::new();
        if let Some(root) = &self.root {
            collect_expanded(root, &mut expanded);
        }
        match repo::discover(&self.watch_root) {
            Ok(mut node) => {
                for path in &expanded {
                    set_node_expanded(&mut node, path);
                }
                self.root = Some(node);
                if !self.selected_is_valid() {
                    self.select_repo(self.watch_root.clone());
                }
            }
            Err(e) => self.error = Some(format!("Cannot reload repositories: {e}")),
        }
    }

    fn selected_is_valid(&self) -> bool {
        self.root
            .as_ref()
            .map(|r| node_is_initialized(r, &self.selected))
            .unwrap_or(false)
    }

    fn after_commit_topology_change(&mut self) {
        self.selected_file = None;
        self.clear_commit_selection();
        self.diff = empty_diff();
        self.reload();
    }

    fn sync_seq_state(&mut self) {
        let kind = match repo::seq_state(&self.selected) {
            repo::SeqState::None => {
                self.seq = None;
                return;
            }
            repo::SeqState::Rebase => SeqKind::Rebase,
            repo::SeqState::RebaseInteractive => SeqKind::RebaseInteractive,
            repo::SeqState::CherryPick => SeqKind::CherryPick,
            repo::SeqState::Revert => SeqKind::Revert,
            repo::SeqState::Merge => SeqKind::Merge,
        };
        let conflicts = repo::seq_conflicts(&self.selected);
        match &mut self.seq {
            Some(s) => {
                s.kind = kind;
                s.conflicts = conflicts;
            }
            None => self.seq = Some(SeqStatus { kind, conflicts }),
        }
    }

    fn after_index_change(&mut self) {
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

    fn refresh_uncommitted_diff(&mut self) {
        if !self.selected_commit.as_ref().is_some_and(|(o, _)| o.is_zero()) {
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
            let sig = hash_diff(&d.rows);
            let changed = sig != self.diff_sig || d.rows.is_empty();
            self.diff = d;
            self.diff_sig = sig;
            if changed {
                self.diff_ver = self.diff_ver.wrapping_add(1);
                self.find.invalidate();
            }
        }
    }
}

fn hash_diff(rows: &[DiffRow]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rows.hash(&mut h);
    h.finish()
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn collect_expanded(node: &repo::RepoNode, out: &mut Vec<PathBuf>) {
    if node.expanded {
        out.push(node.path.clone());
    }
    for child in &node.children {
        collect_expanded(child, out);
    }
}

fn set_node_expanded(node: &mut repo::RepoNode, path: &Path) -> bool {
    if node.path == path {
        node.expanded = true;
        return true;
    }
    node.children
        .iter_mut()
        .any(|c| set_node_expanded(c, path))
}

fn node_is_initialized(node: &repo::RepoNode, path: &Path) -> bool {
    if node.path == path {
        return node.initialized;
    }
    node.children.iter().any(|c| node_is_initialized(c, path))
}

fn find_submodule_parent(
    node: &repo::RepoNode,
    target: &Path,
) -> Option<(PathBuf, String)> {
    for child in &node.children {
        if child.path == target {
            return Some((node.path.clone(), child.name.clone()));
        }
        if let Some(found) = find_submodule_parent(child, target) {
            return Some(found);
        }
    }
    None
}

fn empty_diff() -> FileDiff {
    FileDiff {
        rows: Vec::new(),
        note: None,
        conflict: false,
        rename: false,
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::ui::draw(self, ui);
    }
}
