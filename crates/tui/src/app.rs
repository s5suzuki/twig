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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphOp {
    CherryPick,
    Revert,
    RebaseOnto,
    CheckoutDetached,
}

impl GraphOp {
    fn question(self, short: &str) -> String {
        match self {
            GraphOp::CherryPick => format!("Cherry-pick {short} onto HEAD? (y/n)"),
            GraphOp::Revert => format!("Revert {short}? (y/n)"),
            GraphOp::RebaseOnto => format!("Rebase current branch onto {short}? (y/n)"),
            GraphOp::CheckoutDetached => format!("Check out {short} (detached HEAD)? (y/n)"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RefTarget {
    Branch(String),
    RemoteBranch(String),
    Tag(String),
}

impl RefTarget {
    fn describe(&self) -> String {
        match self {
            RefTarget::Branch(n) => format!("branch {n}"),
            RefTarget::RemoteBranch(n) => format!("remote {n}"),
            RefTarget::Tag(n) => format!("tag {n}"),
        }
    }
}

fn short_oid(oid: &Oid) -> String {
    let hex = oid.to_string();
    hex[..7.min(hex.len())].to_string()
}

fn numbered(refs: &[RefTarget]) -> String {
    refs.iter()
        .enumerate()
        .map(|(i, r)| format!("{}) {}", i + 1, r.describe()))
        .collect::<Vec<_>>()
        .join("  ")
}

fn pick<T: Clone>(items: &[T], c: char) -> Option<T> {
    let idx = c.to_digit(10)? as usize;
    (1..=items.len())
        .contains(&idx)
        .then(|| items[idx - 1].clone())
}

fn fold_ascii(s: &str) -> String {
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

fn row_contains(row: &repo::DiffRow, query: &str) -> bool {
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Prompt {
    Commit,
    Amend,
    ConfirmAmendPushed,
    ConfirmDiscardFiles {
        paths: Vec<String>,
        label: String,
    },
    ConfirmDiscardLines {
        path: String,
        lo: usize,
        hi: usize,
    },
    CreateBranch {
        at: Oid,
    },
    RenameBranch {
        from: String,
    },
    CreateTag {
        at: Oid,
    },
    Reset {
        oid: Oid,
    },
    ConfirmResetHard {
        oid: Oid,
    },
    ConfirmOp {
        op: GraphOp,
        oid: Oid,
    },
    Checkout {
        oid: Oid,
        refs: Vec<RefTarget>,
    },
    DeleteRef {
        refs: Vec<RefTarget>,
    },
    ConfirmDeleteRef {
        target: RefTarget,
    },
    PickRenameBranch {
        names: Vec<String>,
    },
    ConfirmForcePush {
        remote: String,
        refspec: String,
    },
    ConfirmSeqAbort,
    StashOp {
        index: usize,
    },
    ConfirmStashDrop {
        index: usize,
    },
    DiffFind,
    EditGraphLimit,
    SearchQuery,
    SearchReplace,
    ConfirmSearchReplace {
        replacement: String,
    },
    ConfirmSubmodule {
        kind: RemoteKind,
        parent: PathBuf,
        name: String,
    },
}

impl Prompt {
    pub fn wants_text(&self) -> bool {
        matches!(
            self,
            Prompt::Commit
                | Prompt::Amend
                | Prompt::CreateBranch { .. }
                | Prompt::RenameBranch { .. }
                | Prompt::CreateTag { .. }
                | Prompt::DiffFind
                | Prompt::EditGraphLimit
                | Prompt::SearchQuery
                | Prompt::SearchReplace
        )
    }

    pub fn wants_popup(&self) -> bool {
        matches!(self, Prompt::Commit | Prompt::Amend) || self.is_confirm() || self.is_choice()
    }

    fn is_confirm(&self) -> bool {
        matches!(
            self,
            Prompt::ConfirmAmendPushed
                | Prompt::ConfirmDiscardFiles { .. }
                | Prompt::ConfirmDiscardLines { .. }
                | Prompt::ConfirmResetHard { .. }
                | Prompt::ConfirmOp { .. }
                | Prompt::ConfirmDeleteRef { .. }
                | Prompt::ConfirmForcePush { .. }
                | Prompt::ConfirmSeqAbort
                | Prompt::ConfirmStashDrop { .. }
                | Prompt::ConfirmSearchReplace { .. }
                | Prompt::ConfirmSubmodule { .. }
        )
    }

    fn is_choice(&self) -> bool {
        matches!(
            self,
            Prompt::Reset { .. }
                | Prompt::Checkout { .. }
                | Prompt::DeleteRef { .. }
                | Prompt::PickRenameBranch { .. }
                | Prompt::StashOp { .. }
        )
    }

    pub fn hint(&self) -> &'static str {
        if self.wants_text() {
            "Enter: confirm   Esc: cancel"
        } else if self.is_choice() {
            "press the highlighted key   Esc: cancel"
        } else {
            "y: confirm   n / Esc: cancel"
        }
    }

    pub fn label(&self) -> String {
        match self {
            Prompt::Commit => "Commit message:".to_string(),
            Prompt::Amend => "Amend message:".to_string(),
            Prompt::ConfirmAmendPushed => "HEAD is already pushed. Amend anyway? (y/n)".to_string(),
            Prompt::ConfirmDiscardFiles { label, .. } => {
                format!("Discard changes to {label}? (y/n)")
            }
            Prompt::ConfirmDiscardLines { path, .. } => {
                format!("Discard selected lines in {path}? (y/n)")
            }
            Prompt::CreateBranch { at } => format!("Branch name at {}:", short_oid(at)),
            Prompt::RenameBranch { from } => format!("Rename branch {from} to:"),
            Prompt::CreateTag { at } => format!("Tag name at {}:", short_oid(at)),
            Prompt::Reset { oid } => format!(
                "Reset HEAD to {}: (s)oft / (m)ixed / (h)ard",
                short_oid(oid)
            ),
            Prompt::ConfirmResetHard { .. } => {
                "Hard reset discards working tree changes. Continue? (y/n)".to_string()
            }
            Prompt::ConfirmOp { op, oid } => op.question(&short_oid(oid)),
            Prompt::Checkout { refs, .. } => {
                format!("Checkout: {}  c) commit (detached)", numbered(refs))
            }
            Prompt::DeleteRef { refs } => format!("Delete: {}", numbered(refs)),
            Prompt::ConfirmDeleteRef { target } => {
                format!("Delete {}? (y/n)", target.describe())
            }
            Prompt::PickRenameBranch { names } => format!(
                "Rename: {}",
                names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| format!("{}) {n}", i + 1))
                    .collect::<Vec<_>>()
                    .join("  ")
            ),
            Prompt::ConfirmForcePush { remote, .. } => {
                format!("Force push current branch to {remote}? (y/n)")
            }
            Prompt::ConfirmSeqAbort => "Abort the in-progress operation? (y/n)".to_string(),
            Prompt::StashOp { index } => {
                format!("stash@{{{index}}}: (p)op / (a)pply / (d)rop")
            }
            Prompt::ConfirmStashDrop { index } => {
                format!("Drop stash@{{{index}}}? (y/n)")
            }
            Prompt::DiffFind => "Find in diff:".to_string(),
            Prompt::EditGraphLimit => "Graph commit limit:".to_string(),
            Prompt::SearchQuery => "Search repository:".to_string(),
            Prompt::SearchReplace => "Replace matches with:".to_string(),
            Prompt::ConfirmSearchReplace { replacement } => {
                format!("Replace all matches with \"{replacement}\"? (y/n)")
            }
            Prompt::ConfirmSubmodule { kind, name, .. } => match kind {
                RemoteKind::SubmoduleInit => {
                    format!("Initialize submodule {name} (clone)? (y/n)")
                }
                _ => format!("Update submodule {name} to the recorded commit? (y/n)"),
            },
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GraphItem {
    Commit(usize),
    Msg(usize),
    File(usize),
}

fn skip_msg(items: &[GraphItem], i: usize, forward: bool) -> usize {
    let is_msg = |j: usize| matches!(items.get(j), Some(GraphItem::Msg(_)));
    if !is_msg(i) {
        return i;
    }
    let ahead = (i + 1..items.len()).find(|&j| !is_msg(j));
    let behind = (0..i).rev().find(|&j| !is_msg(j));
    if forward {
        ahead.or(behind).unwrap_or(i)
    } else {
        behind.or(ahead).unwrap_or(i)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChangesItem {
    Group {
        staged: bool,
    },
    Folder {
        name: String,
        path: String,
        staged: bool,
        open: bool,
        depth: usize,
    },
    File {
        path: String,
        staged: bool,
        depth: usize,
    },
    StashHeader,
    Stash(usize),
}

impl ChangesItem {
    pub fn depth(&self) -> usize {
        match self {
            ChangesItem::Group { .. } | ChangesItem::StashHeader => 0,
            ChangesItem::Folder { depth, .. } | ChangesItem::File { depth, .. } => *depth,
            ChangesItem::Stash(_) => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteKind {
    Fetch,
    Pull,
    Push,
    ForcePush,
    DeleteRemote,
    SubmoduleInit,
    SubmoduleUpdate,
}

impl RemoteKind {
    pub fn verb(self) -> &'static str {
        match self {
            RemoteKind::Fetch => "fetch",
            RemoteKind::Pull => "pull",
            RemoteKind::Push => "push",
            RemoteKind::ForcePush => "force push",
            RemoteKind::DeleteRemote => "delete remote branch",
            RemoteKind::SubmoduleInit => "submodule init",
            RemoteKind::SubmoduleUpdate => "submodule update",
        }
    }

    pub fn running(self) -> &'static str {
        match self {
            RemoteKind::Fetch => "Fetching",
            RemoteKind::Pull => "Pulling",
            RemoteKind::Push | RemoteKind::ForcePush => "Pushing",
            RemoteKind::DeleteRemote => "Deleting remote branch",
            RemoteKind::SubmoduleInit => "Initializing submodule",
            RemoteKind::SubmoduleUpdate => "Updating submodule",
        }
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub hits: Vec<twit_core::search::FileHit>,
    pub cursor: usize,
    pub scroll: usize,
    pub view_rows: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SearchRow {
    File(usize),
    Line(usize, usize),
}

impl SearchState {
    pub fn rows(&self) -> Vec<SearchRow> {
        let mut out = Vec::new();
        for (i, f) in self.hits.iter().enumerate() {
            out.push(SearchRow::File(i));
            for j in 0..f.lines.len() {
                out.push(SearchRow::Line(i, j));
            }
        }
        out
    }

    pub fn match_count(&self) -> usize {
        self.hits
            .iter()
            .flat_map(|f| f.lines.iter())
            .map(|l| l.ranges.len())
            .sum()
    }
}

enum RemoteMsg {
    Progress(usize, usize),
    Done(Result<repo::SeqOutcome, String>),
}

pub struct RemoteJob {
    pub kind: RemoteKind,
    pub progress: Option<(usize, usize)>,
    rx: Receiver<RemoteMsg>,
}

pub fn seq_label(kind: repo::SeqState) -> &'static str {
    match kind {
        repo::SeqState::Rebase => "Rebase",
        repo::SeqState::RebaseInteractive => "Interactive rebase",
        repo::SeqState::CherryPick => "Cherry-pick",
        repo::SeqState::Revert => "Revert",
        repo::SeqState::Merge => "Merge",
        repo::SeqState::None => "",
    }
}

type Snapshot = (
    PathBuf,
    Option<(String, bool)>,
    Option<Oid>,
    Option<String>,
    Tab,
);

const NAV_HISTORY_MAX: usize = 100;

#[derive(Clone, PartialEq)]
enum NavSel {
    None,
    File { path: String, staged: bool },
    Commit { oid: Oid },
    CommitFile { oid: Oid, path: String },
}

#[derive(Clone)]
struct NavPoint {
    repo: PathBuf,
    tab: Tab,
    focus: Pane,
    sel: NavSel,
    diff_cursor: usize,
}

impl NavPoint {
    fn same_place(&self, other: &NavPoint) -> bool {
        self.repo == other.repo && self.tab == other.tab && self.sel == other.sel
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
    pub pending_editor: Option<PathBuf>,
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
    pub pending_open: Option<(PathBuf, std::time::Instant)>,
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
                _ => Tab::Graph,
            },
            view_mode,
            session: None,
            quit_broadcast: true,
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

    pub fn changes_items(&self) -> Vec<ChangesItem> {
        let mut out = Vec::new();
        out.push(ChangesItem::Group { staged: true });
        self.push_side_items(true, &mut out);
        out.push(ChangesItem::Group { staged: false });
        self.push_side_items(false, &mut out);
        if !self.stashes.is_empty() {
            out.push(ChangesItem::StashHeader);
            out.extend(self.stashes.iter().map(|s| ChangesItem::Stash(s.index)));
        }
        out
    }

    fn push_side_items(&self, staged: bool, out: &mut Vec<ChangesItem>) {
        let entries = if staged { &self.staged } else { &self.unstaged };
        let files: Vec<CommitFile> = entries
            .iter()
            .map(|e| CommitFile {
                path: e.path.clone(),
                kind: e.kind,
            })
            .collect();
        let folded: HashSet<String> = self
            .changes_folds
            .iter()
            .filter(|(s, _)| *s == staged)
            .map(|(_, p)| p.clone())
            .collect();
        for row in repo::commit_file_rows(&files, true, &folded) {
            out.push(match row.kind {
                repo::CommitRowKind::Folder { name, path, open } => ChangesItem::Folder {
                    name,
                    path,
                    staged,
                    open,
                    depth: row.depth + 1,
                },
                repo::CommitRowKind::File(i) => ChangesItem::File {
                    path: files[i].path.clone(),
                    staged,
                    depth: row.depth + 1,
                },
            });
        }
    }

    fn clamp_changes_cursor(&mut self) {
        let n = self.changes_items().len();
        self.changes_cursor = self.changes_cursor.min(n.saturating_sub(1));
    }

    pub fn graph_items(&self) -> Vec<GraphItem> {
        let mut out = Vec::with_capacity(self.graph.rows.len() + self.commit_files.len());
        for (i, row) in self.graph.rows.iter().enumerate() {
            out.push(GraphItem::Commit(i));
            if self.selected_commit == Some(row.id) {
                if !self.commit_detail.is_empty() {
                    for m in 0..=self.commit_detail.len() {
                        out.push(GraphItem::Msg(m));
                    }
                }
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

    fn open_file_diff(&mut self, path: String, staged: bool) {
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
        if oid.is_zero() {
            self.open_uncommitted(oid);
            return;
        }
        match repo::commit_diff(&self.selected, oid) {
            Ok(d) => {
                self.diff = d;
                self.selected_commit = Some(oid);
                self.selected_commit_file = None;
                self.commit_files = repo::commit_files(&self.selected, oid).unwrap_or_default();
                self.load_commit_detail(oid);
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

    fn open_uncommitted(&mut self, oid: Oid) {
        self.commit_files = self.uncommitted_files();
        self.commit_detail.clear();
        self.diff = FileDiff {
            note: Some("Select a file".to_string()),
            ..FileDiff::empty()
        };
        self.selected_commit = Some(oid);
        self.selected_commit_file = None;
        self.selected_file = None;
        self.diff_hl = DiffHighlighter::default();
        self.diff_sig = 0;
        self.diff_nav.reset();
        self.diff_scroll = 0;
        self.diff_center = false;
        self.active_tab = Tab::Diff;
        self.focus = Pane::RightTab;
        self.error = None;
    }

    fn uncommitted_files(&self) -> Vec<CommitFile> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for e in self.unstaged.iter().chain(self.staged.iter()) {
            if seen.insert(e.path.clone()) {
                out.push(CommitFile {
                    path: e.path.clone(),
                    kind: e.kind,
                });
            }
        }
        out
    }

    fn worktree_file_staged(&self, file: &str) -> bool {
        !self.unstaged.iter().any(|e| e.path == file) && self.staged.iter().any(|e| e.path == file)
    }

    fn open_commit_file_diff(&mut self, oid: Oid, path: String) {
        let result = if oid.is_zero() {
            let staged = self.worktree_file_staged(&path);
            repo::file_diff(&self.selected, &path, diff_mode(staged))
        } else {
            repo::commit_file_diff(&self.selected, oid, &path)
        };
        match result {
            Ok(d) => {
                self.diff = d;
                self.selected_commit = Some(oid);
                self.selected_commit_file = Some(path.clone());
                self.load_commit_detail(oid);
                if self.commit_files.is_empty() {
                    self.commit_files = if oid.is_zero() {
                        self.uncommitted_files()
                    } else {
                        repo::commit_files(&self.selected, oid).unwrap_or_default()
                    };
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

    fn load_commit_detail(&mut self, oid: Oid) {
        self.commit_detail = if oid.is_zero() {
            Vec::new()
        } else {
            repo::commit_message(&self.selected, oid)
                .map(|m| {
                    let mut lines: Vec<String> =
                        m.trim_end().lines().skip(1).map(str::to_string).collect();
                    while lines.first().is_some_and(|l| l.trim().is_empty()) {
                        lines.remove(0);
                    }
                    lines
                })
                .unwrap_or_default()
        };
    }

    fn collapse_commit(&mut self) {
        self.selected_commit = None;
        self.selected_commit_file = None;
        self.commit_files.clear();
        self.commit_detail.clear();
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

    fn handle_settings_key(&mut self, ev: KeyEvent) {
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

    fn activate_setting(&mut self) {
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

    fn handle_help_key(&mut self, ev: KeyEvent) {
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

    fn handle_key(&mut self, nk: (Modifiers, Key)) {
        let mut queue = KeyQueue(vec![nk]);

        if queue.take(Modifiers::NONE, Key::Q) || queue.take(Modifiers::CTRL, Key::C) {
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
                        | Action::OpenSearch
                        | Action::NavBack
                        | Action::NavForward
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

    fn open_search_tab(&mut self) {
        self.active_tab = Tab::Search;
        self.focus = Pane::RightTab;
        self.prompt = Some((Prompt::SearchQuery, self.search.query.clone()));
    }

    fn handle_key_single(&mut self, view: View, queue: &mut KeyQueue) {
        let before = self.selection_snapshot();

        let nav = self
            .keymap
            .resolve(queue, Context::Global, &mut self.pending_prefix, |a| {
                matches!(a, Action::NavBack | Action::NavForward)
            });
        for a in nav {
            match a {
                Action::NavBack => {
                    self.nav_go_back();
                }
                Action::NavForward => {
                    self.nav_go_forward();
                }
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
                        self.open_in_embedded(Path::new(&file));
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

    fn current_nav_point(&self) -> NavPoint {
        let sel = if let Some((path, staged)) = &self.selected_file {
            NavSel::File {
                path: path.clone(),
                staged: *staged,
            }
        } else if let (Some(oid), Some(path)) = (&self.selected_commit, &self.selected_commit_file)
        {
            NavSel::CommitFile {
                oid: *oid,
                path: path.clone(),
            }
        } else if let Some(oid) = &self.selected_commit {
            NavSel::Commit { oid: *oid }
        } else {
            NavSel::None
        };
        NavPoint {
            repo: self.selected.clone(),
            tab: self.active_tab,
            focus: self.focus,
            sel,
            diff_cursor: self.diff_nav.cursor,
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
        if self.selected != p.repo {
            self.select_repo(p.repo.clone());
        }
        match p.sel.clone() {
            NavSel::None => {
                self.selected_file = None;
                self.selected_commit = None;
                self.selected_commit_file = None;
                self.commit_files.clear();
                self.commit_detail.clear();
                self.diff = FileDiff::empty();
                self.diff_hl = DiffHighlighter::default();
                self.diff_sig = 0;
                self.diff_nav.reset();
                self.diff_scroll = 0;
            }
            NavSel::File { path, staged } => self.open_file_diff(path, staged),
            NavSel::Commit { oid } => self.open_commit_diff(oid),
            NavSel::CommitFile { oid, path } => self.open_commit_file_diff(oid, path),
        }
        self.active_tab = p.tab;
        self.focus = p.focus;
        if matches!(p.sel, NavSel::File { .. } | NavSel::CommitFile { .. }) {
            let last = self.diff.rows.len().saturating_sub(1);
            self.diff_nav.cursor = p.diff_cursor.min(last);
        }
        self.nav_current = Some(self.current_nav_point());
    }

    pub fn nav_go_back(&mut self) -> bool {
        let Some(prev) = self.nav_back.pop() else {
            return false;
        };
        if let Some(cur) = self.nav_current.take() {
            self.nav_fwd.push(cur);
        }
        self.restore_nav(prev);
        true
    }

    pub fn nav_go_forward(&mut self) -> bool {
        let Some(next) = self.nav_fwd.pop() else {
            return false;
        };
        if let Some(cur) = self.nav_current.take() {
            self.nav_back.push(cur);
        }
        self.restore_nav(next);
        true
    }

    fn handle_prompt_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        let Some((kind, input)) = self.prompt.as_mut() else {
            return;
        };
        if ev.modifiers.contains(KeyModifiers::CONTROL) {
            if ev.code == KeyCode::Char('c') {
                self.prompt = None;
            }
            return;
        }
        if kind.wants_text() {
            match ev.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Enter => self.submit_prompt(),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => input.push(c),
                _ => {}
            }
        } else if kind.is_choice() {
            match ev.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Char(c) => self.handle_choice(c),
                _ => {}
            }
        } else {
            match ev.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.submit_prompt(),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.prompt = None,
                _ => {}
            }
        }
    }

    fn handle_choice(&mut self, c: char) {
        let Some((kind, _)) = self.prompt.clone() else {
            return;
        };
        match kind {
            Prompt::Reset { oid } => match c {
                's' => self.take_prompt_and(|app| app.run_reset(oid, repo::ResetMode::Soft)),
                'm' => self.take_prompt_and(|app| app.run_reset(oid, repo::ResetMode::Mixed)),
                'h' => self.prompt = Some((Prompt::ConfirmResetHard { oid }, String::new())),
                _ => {}
            },
            Prompt::Checkout { oid, refs } => {
                if c == 'c' {
                    self.prompt = Some((
                        Prompt::ConfirmOp {
                            op: GraphOp::CheckoutDetached,
                            oid,
                        },
                        String::new(),
                    ));
                } else if let Some(target) = pick(&refs, c) {
                    self.take_prompt_and(|app| app.run_checkout_ref(&target));
                }
            }
            Prompt::DeleteRef { refs } => {
                if let Some(target) = pick(&refs, c) {
                    self.prompt = Some((Prompt::ConfirmDeleteRef { target }, String::new()));
                }
            }
            Prompt::PickRenameBranch { names } => {
                if let Some(from) = pick(&names, c) {
                    self.prompt = Some((Prompt::RenameBranch { from: from.clone() }, from));
                }
            }
            Prompt::StashOp { index } => match c {
                'p' => self.take_prompt_and(|app| {
                    app.run_stash_op(repo::stash_pop(&app.selected, index), "stash pop")
                }),
                'a' => self.take_prompt_and(|app| {
                    app.run_stash_op(repo::stash_apply(&app.selected, index), "stash apply")
                }),
                'd' => self.prompt = Some((Prompt::ConfirmStashDrop { index }, String::new())),
                _ => {}
            },
            _ => {}
        }
    }

    fn take_prompt_and(&mut self, f: impl FnOnce(&mut Self)) {
        self.prompt = None;
        f(self);
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
            Prompt::CreateBranch { at } => {
                self.run_ref_op(|s, name| repo::create_branch(&s.selected, name, at), &input)
            }
            Prompt::RenameBranch { from } => self.run_ref_op(
                |s, name| repo::rename_branch(&s.selected, &from, name),
                &input,
            ),
            Prompt::CreateTag { at } => {
                self.run_ref_op(|s, name| repo::create_tag(&s.selected, name, at), &input)
            }
            Prompt::ConfirmResetHard { oid } => self.run_reset(oid, repo::ResetMode::Hard),
            Prompt::ConfirmOp { op, oid } => self.run_graph_op(op, oid),
            Prompt::ConfirmDeleteRef { target } => self.run_delete_ref(&target),
            Prompt::ConfirmForcePush { remote, refspec } => {
                self.start_remote(RemoteKind::ForcePush, Some(remote), vec![refspec]);
            }
            Prompt::ConfirmSeqAbort => self.seq_abort(),
            Prompt::ConfirmStashDrop { index } => {
                self.run_stash_op(repo::stash_drop(&self.selected, index), "stash drop");
            }
            Prompt::DiffFind => {
                let query = input.trim().to_string();
                if query.is_empty() {
                    self.diff_find = None;
                } else {
                    let on_match = self
                        .diff
                        .rows
                        .get(self.diff_nav.cursor)
                        .is_some_and(|r| row_contains(r, &query));
                    self.diff_find = Some(query);
                    if !on_match {
                        self.jump_find(true);
                    }
                }
            }
            Prompt::EditGraphLimit => match input.trim().parse::<usize>() {
                Ok(n) if n > 0 => {
                    self.config.graph_commit_limit = n;
                    self.config.save();
                    self.refresh();
                }
                _ => self.error = Some(format!("invalid commit limit: {}", input.trim())),
            },
            Prompt::SearchQuery => self.run_search(input.trim()),
            Prompt::SearchReplace => {
                self.prompt = Some((
                    Prompt::ConfirmSearchReplace { replacement: input },
                    String::new(),
                ));
            }
            Prompt::ConfirmSearchReplace { replacement } => {
                self.run_search_replace(&replacement);
            }
            Prompt::ConfirmSubmodule { kind, parent, name } => {
                self.start_remote_submodule(kind, parent, name);
            }
            Prompt::Reset { .. }
            | Prompt::Checkout { .. }
            | Prompt::DeleteRef { .. }
            | Prompt::PickRenameBranch { .. }
            | Prompt::StashOp { .. } => {}
        }
    }

    fn run_ref_op(
        &mut self,
        f: impl FnOnce(&Self, &str) -> Result<(), twit_core::git2::Error>,
        input: &str,
    ) {
        let name = input.trim();
        if name.is_empty() {
            return;
        }
        match f(self, name) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("ref op failed: {e}")),
        }
        self.refresh();
    }

    fn run_reset(&mut self, oid: Oid, mode: repo::ResetMode) {
        match repo::reset(&self.selected, oid, mode) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("reset failed: {e}")),
        }
        self.refresh();
    }

    fn run_graph_op(&mut self, op: GraphOp, oid: Oid) {
        match op {
            GraphOp::CherryPick => {
                let r = repo::cherry_pick(&self.selected, oid);
                self.apply_seq_outcome("cherry-pick", r);
            }
            GraphOp::Revert => {
                let r = repo::revert(&self.selected, oid);
                self.apply_seq_outcome("revert", r);
            }
            GraphOp::RebaseOnto => {
                let r = repo::rebase_onto(&self.selected, oid);
                self.apply_seq_outcome("rebase", r);
            }
            GraphOp::CheckoutDetached => {
                match repo::checkout_commit(&self.selected, oid) {
                    Ok(()) => self.error = None,
                    Err(e) => self.error = Some(format!("checkout failed: {e}")),
                }
                self.refresh();
            }
        }
    }

    fn apply_seq_outcome(
        &mut self,
        what: &str,
        r: Result<repo::SeqOutcome, twit_core::git2::Error>,
    ) {
        match r {
            Ok(_) => self.error = None,
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.refresh();
    }

    fn run_checkout_ref(&mut self, target: &RefTarget) {
        let res = match target {
            RefTarget::Branch(name) => repo::checkout_branch(&self.selected, name),
            RefTarget::RemoteBranch(name) => repo::checkout_tracking(&self.selected, name),
            RefTarget::Tag(_) => return,
        };
        match res {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("checkout failed: {e}")),
        }
        self.refresh();
    }

    fn run_delete_ref(&mut self, target: &RefTarget) {
        let res = match target {
            RefTarget::Branch(name) => repo::delete_branch(&self.selected, name),
            RefTarget::Tag(name) => repo::delete_tag(&self.selected, name),
            RefTarget::RemoteBranch(name) => {
                let Some((remote, branch)) = name.split_once('/') else {
                    self.error = Some(format!("invalid remote branch: {name}"));
                    return;
                };
                let (remote, branch) = (remote.to_string(), branch.to_string());
                self.start_remote(RemoteKind::DeleteRemote, Some(remote), vec![branch]);
                return;
            }
        };
        match res {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("delete failed: {e}")),
        }
        self.refresh();
    }

    fn seq_continue(&mut self) {
        let Some((kind, _)) = self.seq else {
            return;
        };
        match kind {
            repo::SeqState::RebaseInteractive => self.seq_git_shell("--continue"),
            repo::SeqState::Rebase => {
                let r = repo::rebase_continue(&self.selected);
                self.apply_seq_outcome("rebase --continue", r);
            }
            repo::SeqState::CherryPick => {
                let r = repo::cherry_pick_continue(&self.selected);
                self.apply_seq_outcome("cherry-pick --continue", r);
            }
            repo::SeqState::Revert => {
                let r = repo::revert_continue(&self.selected);
                self.apply_seq_outcome("revert --continue", r);
            }
            repo::SeqState::Merge => {
                let r = repo::merge_continue(&self.selected);
                self.apply_seq_outcome("merge --continue", r);
            }
            repo::SeqState::None => {}
        }
    }

    fn seq_abort(&mut self) {
        let Some((kind, _)) = self.seq else {
            return;
        };
        let res = match kind {
            repo::SeqState::RebaseInteractive => {
                self.seq_git_shell("--abort");
                return;
            }
            repo::SeqState::Rebase => repo::rebase_abort(&self.selected),
            repo::SeqState::CherryPick => repo::cherry_pick_abort(&self.selected),
            repo::SeqState::Revert => repo::revert_abort(&self.selected),
            repo::SeqState::Merge => repo::merge_abort(&self.selected),
            repo::SeqState::None => return,
        };
        if let Err(e) = res {
            self.error = Some(format!("abort failed: {e}"));
        } else {
            self.error = None;
        }
        self.refresh();
    }

    fn seq_git_shell(&mut self, arg: &str) {
        self.pending_shell = Some(vec![
            "git".to_string(),
            "-C".to_string(),
            self.selected.to_string_lossy().into_owned(),
            "rebase".to_string(),
            arg.to_string(),
        ]);
    }

    fn push(&mut self, force: bool) {
        let remote = repo::primary_remote(&self.selected);
        let Some(refspec) = repo::head_push_refspec(&self.selected) else {
            self.error = Some("not on a branch to push".to_string());
            return;
        };
        if force {
            let Some(remote) = remote else {
                self.error = Some("no remote configured".to_string());
                return;
            };
            self.prompt = Some((Prompt::ConfirmForcePush { remote, refspec }, String::new()));
            return;
        }
        self.start_remote(RemoteKind::Push, remote, vec![refspec]);
    }

    fn submodule_prompt(&mut self, row: &SidebarRow, update: bool) {
        let Some(parent) = row.parent.clone() else {
            self.error = Some("not a submodule".to_string());
            return;
        };
        if update && !row.initialized {
            self.error = Some("initialize the submodule first".to_string());
            return;
        }
        if !update && row.initialized {
            self.error = Some("submodule is already initialized".to_string());
            return;
        }
        let kind = if update {
            RemoteKind::SubmoduleUpdate
        } else {
            RemoteKind::SubmoduleInit
        };
        self.prompt = Some((
            Prompt::ConfirmSubmodule {
                kind,
                parent,
                name: row.name.clone(),
            },
            String::new(),
        ));
    }

    fn start_remote_submodule(&mut self, kind: RemoteKind, parent: PathBuf, name: String) {
        if self.remote.is_some() {
            self.error = Some("a remote operation is already running".to_string());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ptx = tx.clone();
            let progress = move |received, total| {
                let _ = ptx.send(RemoteMsg::Progress(received, total));
            };
            let result = match kind {
                RemoteKind::SubmoduleUpdate => repo::submodule_update(&parent, &name, progress),
                _ => repo::submodule_init(&parent, &name, progress),
            }
            .map(|()| repo::SeqOutcome::Done);
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
        });
        self.error = None;
        self.remote = Some(RemoteJob {
            kind,
            progress: None,
            rx,
        });
    }

    fn rediscover(&mut self) {
        let root_path = self.root.path.clone();
        match repo::discover(&root_path) {
            Ok(node) => self.root = node,
            Err(e) => self.error = Some(format!("rediscover failed: {e}")),
        }
    }

    fn start_remote(&mut self, kind: RemoteKind, remote: Option<String>, refspecs: Vec<String>) {
        if self.remote.is_some() {
            self.error = Some("a remote operation is already running".to_string());
            return;
        }
        let Some(remote) = remote else {
            self.error = Some("no remote configured".to_string());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let path = self.selected.clone();
        let refspecs = if kind == RemoteKind::ForcePush {
            refspecs
                .into_iter()
                .map(|r| {
                    if r.starts_with('+') {
                        r
                    } else {
                        format!("+{r}")
                    }
                })
                .collect()
        } else {
            refspecs
        };
        std::thread::spawn(move || {
            let ptx = tx.clone();
            let progress = move |received, total| {
                let _ = ptx.send(RemoteMsg::Progress(received, total));
            };
            let result = match kind {
                RemoteKind::Fetch => {
                    repo::fetch(&path, &remote, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::Pull => repo::pull(&path, &remote, progress),
                RemoteKind::Push | RemoteKind::ForcePush => {
                    repo::push(&path, &remote, &refspecs, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::DeleteRemote => {
                    let branch = refspecs.first().cloned().unwrap_or_default();
                    repo::delete_remote_branch(&path, &remote, &branch, progress)
                        .map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate => Err(
                    twit_core::git2::Error::from_str("not a plain remote operation"),
                ),
            };
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
        });
        self.error = None;
        self.remote = Some(RemoteJob {
            kind,
            progress: None,
            rx,
        });
    }

    pub fn poll_remote(&mut self) -> bool {
        let Some(job) = self.remote.as_mut() else {
            return false;
        };
        let mut dirty = false;
        let mut done = None;
        loop {
            match job.rx.try_recv() {
                Ok(RemoteMsg::Progress(received, total)) => {
                    job.progress = Some((received, total));
                    dirty = true;
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
            let kind = self
                .remote
                .take()
                .map(|j| j.kind)
                .unwrap_or(RemoteKind::Fetch);
            match res {
                Ok(_) => {
                    self.error = None;
                    if matches!(
                        kind,
                        RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate
                    ) {
                        self.rediscover();
                    }
                }
                Err(e) => self.error = Some(format!("{} failed: {e}", kind.verb())),
            }
            self.refresh();
            dirty = true;
        }
        dirty
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

    fn changes_keys(&mut self, queue: &mut KeyQueue) {
        let half = (self.changes_view_rows / 2).max(1);
        let actions =
            self.keymap
                .resolve(queue, Context::Changes, &mut self.pending_prefix, |_| true);
        for a in actions {
            let items = self.changes_items();
            let last = items.len().saturating_sub(1);
            let cursor = self.changes_cursor.min(last);
            let item = items.get(cursor).cloned();
            match a {
                Action::ChangesDown => self.changes_cursor = (cursor + 1).min(last),
                Action::ChangesUp => self.changes_cursor = cursor.saturating_sub(1),
                Action::ChangesTop => self.changes_cursor = 0,
                Action::ChangesBottom => self.changes_cursor = last,
                Action::ChangesHalfPageDown => self.changes_cursor = (cursor + half).min(last),
                Action::ChangesHalfPageUp => self.changes_cursor = cursor.saturating_sub(half),
                Action::ChangesActivate => match item {
                    Some(ChangesItem::Folder {
                        path, staged, open, ..
                    }) => {
                        self.set_fold(staged, path, open);
                    }
                    item => self.changes_open(item),
                },
                Action::ChangesExpand => match item {
                    Some(ChangesItem::Folder {
                        path,
                        staged,
                        open: false,
                        ..
                    }) => self.set_fold(staged, path, false),
                    Some(ChangesItem::Folder { .. })
                    | Some(ChangesItem::Group { .. })
                    | Some(ChangesItem::StashHeader) => {
                        self.changes_cursor = (cursor + 1).min(last)
                    }
                    item => self.changes_open(item),
                },
                Action::ChangesCollapse => match item {
                    Some(ChangesItem::Folder {
                        path,
                        staged,
                        open: true,
                        ..
                    }) => self.set_fold(staged, path, true),
                    Some(it) => {
                        let depth = it.depth();
                        if depth > 0
                            && let Some(p) = (0..cursor).rev().find(|&i| {
                                items[i].depth() < depth
                                    && matches!(
                                        items[i],
                                        ChangesItem::Folder { .. }
                                            | ChangesItem::Group { .. }
                                            | ChangesItem::StashHeader
                                    )
                            })
                        {
                            self.changes_cursor = p;
                        }
                    }
                    None => {}
                },
                Action::ChangesStageToggle => match item {
                    Some(ChangesItem::File { path, staged, .. }) => {
                        self.stage_paths(staged, vec![path])
                    }
                    Some(ChangesItem::Folder { path, staged, .. }) => {
                        let paths = self.side_paths(staged, Some(&path));
                        self.stage_paths(staged, paths);
                    }
                    Some(ChangesItem::Group { staged }) => {
                        let paths = self.side_paths(staged, None);
                        self.stage_paths(staged, paths);
                    }
                    Some(ChangesItem::Stash(index)) => {
                        self.prompt = Some((Prompt::StashOp { index }, String::new()));
                    }
                    _ => {}
                },
                Action::ChangesEdit => {
                    if let Some(ChangesItem::File { path, .. }) = item {
                        self.pending_editor = Some(self.selected.join(path));
                    }
                }
                Action::ChangesDiscard => self.changes_discard(item),
                _ => {}
            }
        }
    }

    fn changes_open(&mut self, item: Option<ChangesItem>) {
        match item {
            Some(ChangesItem::File { path, staged, .. }) => {
                self.open_file_diff(path, staged);
                self.pending_focus_jump = self.error.is_none();
            }
            Some(ChangesItem::Stash(index)) => {
                if let Some(oid) = self
                    .stashes
                    .iter()
                    .find(|s| s.index == index)
                    .map(|s| s.oid)
                {
                    self.open_commit_diff(oid);
                    self.pending_focus_jump = self.error.is_none();
                }
            }
            _ => {}
        }
    }

    fn set_fold(&mut self, staged: bool, path: String, folded: bool) {
        if folded {
            self.changes_folds.insert((staged, path));
        } else {
            self.changes_folds.remove(&(staged, path));
        }
        self.clamp_changes_cursor();
    }

    fn side_paths(&self, staged: bool, folder: Option<&str>) -> Vec<String> {
        let entries = if staged { &self.staged } else { &self.unstaged };
        let prefix = folder.map(|p| format!("{p}/"));
        entries
            .iter()
            .filter(|e| prefix.as_deref().is_none_or(|p| e.path.starts_with(p)))
            .map(|e| e.path.clone())
            .collect()
    }

    fn changes_discard(&mut self, item: Option<ChangesItem>) {
        let folder = match item {
            Some(ChangesItem::File {
                path,
                staged: false,
                ..
            }) => {
                let mut paths = vec![path.clone()];
                if let Some(old) = self
                    .unstaged
                    .iter()
                    .find(|e| e.path == path)
                    .and_then(|e| e.old_path.clone())
                {
                    paths.push(old);
                }
                if !self.config.confirm_discard {
                    self.run_discard_files(&paths);
                    return;
                }
                self.prompt = Some((
                    Prompt::ConfirmDiscardFiles { paths, label: path },
                    String::new(),
                ));
                return;
            }
            Some(ChangesItem::Folder {
                path,
                staged: false,
                ..
            }) => Some(path.clone()),
            Some(ChangesItem::Group { staged: false }) => None,
            _ => return,
        };
        let files = self.side_paths(false, folder.as_deref());
        if files.is_empty() {
            return;
        }
        let label = match &folder {
            Some(path) => format!("{path}/ ({} files)", files.len()),
            None => format!("all {} files", files.len()),
        };
        let paths = self
            .unstaged
            .iter()
            .filter(|e| files.contains(&e.path))
            .flat_map(|e| std::iter::once(e.path.clone()).chain(e.old_path.clone()))
            .collect::<Vec<_>>();
        if !self.config.confirm_discard {
            self.run_discard_files(&paths);
            return;
        }
        self.prompt = Some((Prompt::ConfirmDiscardFiles { paths, label }, String::new()));
    }

    fn stash_push(&mut self) {
        if self.staged.is_empty() && self.unstaged.is_empty() {
            self.error = Some("nothing to stash".to_string());
            return;
        }
        match repo::stash_save(&self.selected, None) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("stash failed: {e}")),
        }
        self.refresh();
    }

    fn run_stash_op(&mut self, r: Result<(), twit_core::git2::Error>, what: &str) {
        match r {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("{what} failed: {e}")),
        }
        self.refresh();
    }

    fn stage_paths(&mut self, staged: bool, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
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
        if !self.config.confirm_discard {
            self.run_discard_lines(&path, lo, hi);
            return;
        }
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
                        | Action::GraphEditor
                        | Action::GraphReset
                        | Action::GraphCreateBranch
                        | Action::GraphCreateTag
                        | Action::GraphCherryPick
                        | Action::GraphRevert
                        | Action::GraphRebaseOnto
                        | Action::GraphRebaseInteractive
                        | Action::GraphCheckout
                        | Action::GraphRenameBranch
                        | Action::GraphDeleteRef
                        | Action::GraphFetch
                        | Action::GraphPull
                        | Action::GraphPush
                        | Action::GraphForcePush
                )
            });
        for a in actions {
            let items = self.graph_items();
            let last = items.len().saturating_sub(1);
            match a {
                Action::GraphDown => {
                    self.graph_cursor = skip_msg(&items, (self.graph_cursor + 1).min(last), true)
                }
                Action::GraphUp => {
                    self.graph_cursor =
                        skip_msg(&items, self.graph_cursor.saturating_sub(1), false)
                }
                Action::GraphTop => self.graph_cursor = 0,
                Action::GraphBottom => self.graph_cursor = skip_msg(&items, last, false),
                Action::GraphHalfPageDown => {
                    self.graph_cursor = skip_msg(&items, (self.graph_cursor + half).min(last), true)
                }
                Action::GraphHalfPageUp => {
                    self.graph_cursor =
                        skip_msg(&items, self.graph_cursor.saturating_sub(half), false)
                }
                Action::GraphOpen => self.graph_open(),
                Action::GraphCollapse => self.graph_collapse(),
                Action::GraphEditor => self.graph_open_editor(),
                Action::GraphReset => self.graph_op_prompt(a),
                Action::GraphCreateBranch => self.graph_op_prompt(a),
                Action::GraphCreateTag => self.graph_op_prompt(a),
                Action::GraphCherryPick => self.graph_op_prompt(a),
                Action::GraphRevert => self.graph_op_prompt(a),
                Action::GraphRebaseOnto => self.graph_op_prompt(a),
                Action::GraphCheckout => self.graph_op_prompt(a),
                Action::GraphRenameBranch => self.graph_op_prompt(a),
                Action::GraphDeleteRef => self.graph_op_prompt(a),
                Action::GraphRebaseInteractive => self.interactive_rebase(),
                Action::GraphFetch => {
                    let remote = repo::primary_remote(&self.selected);
                    self.start_remote(RemoteKind::Fetch, remote, Vec::new());
                }
                Action::GraphPull => {
                    if !self.busy_with_seq() {
                        let remote = repo::primary_remote(&self.selected);
                        self.start_remote(RemoteKind::Pull, remote, Vec::new());
                    }
                }
                Action::GraphPush => self.push(false),
                Action::GraphForcePush => self.push(true),
                _ => {}
            }
        }
    }

    fn graph_target(&self) -> Option<(usize, Oid)> {
        let items = self.graph_items();
        let cursor = self.graph_cursor.min(items.len().checked_sub(1)?);
        let row = match items.get(cursor)? {
            GraphItem::Commit(r) => *r,
            GraphItem::Msg(_) | GraphItem::File(_) => {
                (0..cursor).rev().find_map(|i| match items[i] {
                    GraphItem::Commit(r) => Some(r),
                    _ => None,
                })?
            }
        };
        let gr = &self.graph.rows[row];
        (!gr.is_uncommitted).then_some((row, gr.id))
    }

    fn row_refs(&self, row: usize) -> Vec<RefTarget> {
        use twit_core::repo::RefKind;
        self.graph.rows[row]
            .refs
            .iter()
            .filter_map(|r| match r.kind {
                RefKind::LocalBranch if !r.is_head => Some(RefTarget::Branch(r.name.clone())),
                RefKind::RemoteBranch => Some(RefTarget::RemoteBranch(r.name.clone())),
                RefKind::Tag => Some(RefTarget::Tag(r.name.clone())),
                _ => None,
            })
            .collect()
    }

    fn busy_with_seq(&mut self) -> bool {
        if repo::seq_state(&self.selected) != repo::SeqState::None {
            self.error = Some("finish or abort the in-progress operation first".to_string());
            true
        } else {
            false
        }
    }

    fn graph_op_prompt(&mut self, action: Action) {
        let Some((row, oid)) = self.graph_target() else {
            return;
        };
        if self.busy_with_seq() {
            return;
        }
        let confirm_op = |op| (Prompt::ConfirmOp { op, oid }, String::new());
        self.prompt = Some(match action {
            Action::GraphReset => (Prompt::Reset { oid }, String::new()),
            Action::GraphCreateBranch => (Prompt::CreateBranch { at: oid }, String::new()),
            Action::GraphCreateTag => (Prompt::CreateTag { at: oid }, String::new()),
            Action::GraphCherryPick => confirm_op(GraphOp::CherryPick),
            Action::GraphRevert => confirm_op(GraphOp::Revert),
            Action::GraphRebaseOnto => confirm_op(GraphOp::RebaseOnto),
            Action::GraphCheckout => {
                let refs: Vec<RefTarget> = self
                    .row_refs(row)
                    .into_iter()
                    .filter(|r| !matches!(r, RefTarget::Tag(_)))
                    .collect();
                if refs.is_empty() {
                    confirm_op(GraphOp::CheckoutDetached)
                } else {
                    (Prompt::Checkout { oid, refs }, String::new())
                }
            }
            Action::GraphRenameBranch => {
                use twit_core::repo::RefKind;
                let names: Vec<String> = self.graph.rows[row]
                    .refs
                    .iter()
                    .filter(|r| r.kind == RefKind::LocalBranch)
                    .map(|r| r.name.clone())
                    .collect();
                match names.len() {
                    0 => {
                        self.error = Some("no local branch on this commit".to_string());
                        return;
                    }
                    1 => {
                        let from = names[0].clone();
                        (Prompt::RenameBranch { from: from.clone() }, from)
                    }
                    _ => (Prompt::PickRenameBranch { names }, String::new()),
                }
            }
            Action::GraphDeleteRef => {
                let refs = self.row_refs(row);
                match refs.len() {
                    0 => {
                        self.error = Some("no branch/tag on this commit".to_string());
                        return;
                    }
                    1 => (
                        Prompt::ConfirmDeleteRef {
                            target: refs[0].clone(),
                        },
                        String::new(),
                    ),
                    _ => (Prompt::DeleteRef { refs }, String::new()),
                }
            }
            _ => return,
        });
    }

    fn search_keys(&mut self, queue: &mut KeyQueue) {
        if queue.take(Modifiers::NONE, Key::Slash) {
            self.prompt = Some((Prompt::SearchQuery, self.search.query.clone()));
            return;
        }
        if queue.take(Modifiers::NONE, Key::R) {
            if self.search.hits.is_empty() {
                self.error = Some("nothing to replace (run a search first)".to_string());
            } else {
                self.prompt = Some((Prompt::SearchReplace, String::new()));
            }
            return;
        }
        let rows = self.search.rows();
        if rows.is_empty() {
            return;
        }
        let last = rows.len() - 1;
        let half = (self.search.view_rows / 2).max(1);
        if queue.take(Modifiers::NONE, Key::J) || queue.take(Modifiers::NONE, Key::ArrowDown) {
            self.search.cursor = (self.search.cursor + 1).min(last);
        }
        if queue.take(Modifiers::NONE, Key::K) || queue.take(Modifiers::NONE, Key::ArrowUp) {
            self.search.cursor = self.search.cursor.saturating_sub(1);
        }
        if queue.take(Modifiers::CTRL, Key::D) {
            self.search.cursor = (self.search.cursor + half).min(last);
        }
        if queue.take(Modifiers::CTRL, Key::U) {
            self.search.cursor = self.search.cursor.saturating_sub(half);
        }
        if queue.take(Modifiers::SHIFT, Key::G) {
            self.search.cursor = last;
        }
        if queue.take(Modifiers::NONE, Key::Enter) || queue.take(Modifiers::NONE, Key::E) {
            let path = match rows.get(self.search.cursor.min(last)) {
                Some(SearchRow::File(i)) | Some(SearchRow::Line(i, _)) => {
                    self.search.hits.get(*i).map(|f| f.path.clone())
                }
                None => None,
            };
            if let Some(path) = path {
                self.pending_editor = Some(self.selected.join(path));
            }
        }
    }

    fn run_search(&mut self, query: &str) {
        self.search.query = query.to_string();
        self.search.cursor = 0;
        self.search.scroll = 0;
        if query.is_empty() {
            self.search.hits.clear();
            return;
        }
        match twit_core::search::Matcher::new(query, false, false) {
            Ok(m) => {
                self.search.hits = twit_core::search::search_repo(&self.selected, &m);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("search failed: {e}")),
        }
    }

    fn run_search_replace(&mut self, replacement: &str) {
        let matcher = match twit_core::search::Matcher::new(&self.search.query, false, false) {
            Ok(m) => m,
            Err(e) => {
                self.error = Some(format!("replace failed: {e}"));
                return;
            }
        };
        let mut files = 0usize;
        let mut count = 0usize;
        for hit in &self.search.hits {
            let abs = self.selected.join(&hit.path);
            let Ok(text) = std::fs::read_to_string(&abs) else {
                continue;
            };
            let (new_text, n) =
                twit_core::search::replace_all_in_text(&matcher, &text, replacement);
            if n > 0 && std::fs::write(&abs, new_text).is_ok() {
                files += 1;
                count += n;
            }
        }
        self.error = Some(format!("replaced {count} matches in {files} files"));
        let query = self.search.query.clone();
        self.run_search(&query);
        self.refresh();
    }

    fn jump_find(&mut self, forward: bool) {
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

    fn arm_diff_recheck(&mut self) {
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

    fn worktree_file_changed(&self, file: &str) -> bool {
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

    fn graph_open_editor(&mut self) {
        let items = self.graph_items();
        if let Some(GraphItem::File(k)) =
            items.get(self.graph_cursor.min(items.len().saturating_sub(1)))
            && let Some(f) = self.commit_files.get(*k)
        {
            self.pending_editor = Some(self.selected.join(&f.path));
        }
    }

    fn interactive_rebase(&mut self) {
        let Some((_, oid)) = self.graph_target() else {
            return;
        };
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
        self.pending_shell = Some(vec![
            "git".to_string(),
            "-C".to_string(),
            self.selected.to_string_lossy().into_owned(),
            "rebase".to_string(),
            "-i".to_string(),
            base,
        ]);
    }

    fn graph_open(&mut self) {
        let items = self.graph_items();
        match items.get(self.graph_cursor.min(items.len().saturating_sub(1))) {
            Some(GraphItem::Commit(row)) => {
                let row = &self.graph.rows[*row];
                if self.selected_commit == Some(row.id) && self.selected_commit_file.is_none() {
                    self.collapse_commit();
                    return;
                }
                let oid = row.id;
                self.open_commit_diff(oid);
                if self.error.is_none() {
                    self.set_graph_cursor_to_commit(oid);
                    if matches!(self.view_mode, ViewMode::All | ViewMode::Single(View::Main)) {
                        self.active_tab = Tab::Graph;
                    }
                }
            }
            Some(GraphItem::File(k)) => {
                if let (Some(oid), Some(f)) = (self.selected_commit, self.commit_files.get(*k)) {
                    let path = f.path.clone();
                    self.open_commit_file_diff(oid, path);
                    self.pending_focus_jump = self.error.is_none();
                }
            }
            Some(GraphItem::Msg(_)) | None => {}
        }
    }

    fn graph_collapse(&mut self) {
        let items = self.graph_items();
        let cursor = self.graph_cursor.min(items.len().saturating_sub(1));
        match items.get(cursor) {
            Some(GraphItem::File(_) | GraphItem::Msg(_)) => {
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
        if let Some(idx) = self
            .graph_items()
            .iter()
            .position(|it| matches!(it, GraphItem::Commit(r) if self.graph.rows[*r].id == oid))
        {
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

    pub fn ensure_editor(&mut self) {
        if self.term.as_mut().is_some_and(|t| t.is_alive()) {
            return;
        }
        match crate::term::EditorTerm::spawn_nvim(&self.nvim_socket, &self.selected) {
            Ok(t) => {
                self.term = Some(t);
                self.error = None;
            }
            Err(e) => {
                self.term = None;
                self.error = Some(format!("nvim spawn failed: {e}"));
            }
        }
    }

    fn editor_focused(&mut self) -> bool {
        self.active_tab == Tab::Editor
            && self.focus == Pane::RightTab
            && matches!(self.view_mode, ViewMode::All | ViewMode::Single(View::Main))
            && self.term.as_mut().is_some_and(|t| t.is_alive())
    }

    fn handle_editor_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        let alt = ev.modifiers.contains(KeyModifiers::ALT);
        match ev.code {
            KeyCode::Char('h') if alt => return self.focus_move(-1),
            KeyCode::Char('l') if alt => return self.focus_move(1),
            KeyCode::Tab => return self.cycle_tab(1),
            KeyCode::BackTab => return self.cycle_tab(-1),
            _ => {}
        }
        if let Some(t) = self.term.as_mut() {
            t.feed_key(&ev);
        }
    }

    pub fn open_in_embedded(&mut self, file: &Path) -> bool {
        if !matches!(self.view_mode, ViewMode::All | ViewMode::Single(View::Main)) {
            return false;
        }
        self.ensure_editor();
        if self.term.is_none() {
            return true;
        }
        self.active_tab = Tab::Editor;
        self.focus = Pane::RightTab;
        if self.nvim_socket.exists() {
            match twit_core::editor::open_abs_in_server(file, &self.nvim_socket) {
                Ok(()) => self.error = None,
                Err(e) => self.error = Some(e),
            }
        } else {
            self.pending_open = Some((
                file.to_path_buf(),
                std::time::Instant::now() + std::time::Duration::from_secs(10),
            ));
        }
        true
    }

    pub fn poll_pending_open(&mut self) -> bool {
        let Some((file, deadline)) = self.pending_open.clone() else {
            return false;
        };
        if !self.term.as_mut().is_some_and(|t| t.is_alive()) {
            self.pending_open = None;
            return true;
        }
        if self.nvim_socket.exists() {
            self.pending_open = None;
            match twit_core::editor::open_abs_in_server(&file, &self.nvim_socket) {
                Ok(()) => self.error = None,
                Err(e) => self.error = Some(e),
            }
            return true;
        }
        if std::time::Instant::now() > deadline {
            self.pending_open = None;
            self.error = Some("nvim did not open its socket".to_string());
            return true;
        }
        false
    }
}

fn nvim_socket_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("twig")
        .join(format!("{}-{n}-nvim.sock", std::process::id()))
}

fn diff_mode(staged: bool) -> repo::DiffMode {
    if staged {
        repo::DiffMode::Staged
    } else {
        repo::DiffMode::Unstaged
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
