use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use git2::Oid;

use crate::config::Config;
use crate::keys::{Chord, Keymap};
use crate::repo::{self, DiffMode, DiffRow, FileDiff, Graph, RepoNode, StatusEntry};

pub const LIST_PAGE: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Graph,
    Diff,
    Editor,
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
    CherryPick,
    Revert,
}

impl SeqKind {
    pub fn label(self) -> &'static str {
        match self {
            SeqKind::Rebase => "Rebase",
            SeqKind::CherryPick => "Cherry-pick",
            SeqKind::Revert => "Revert",
        }
    }
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
    pub selected_commit_file: Option<String>,
    pub diff: FileDiff,
    pub diff_cursor: usize,
    pub diff_anchor: Option<usize>,
    pub diff_scroll_pending: bool,
    pub diff_scrolled_prev: bool,
    pub diff_visible: Option<(usize, usize)>,
    pub commit_msg: String,
    pub active_tab: Tab,

    pub term: Option<crate::term::Term>,
    pub nvim_socket: PathBuf,

    pub pending_open: Option<PathBuf>,

    pub shell: Option<crate::term::Term>,
    pub shell_open: bool,

    pub focus: Pane,
    pub changes_cursor: usize,
    pub sidebar_cursor: usize,
    pub keymap: Keymap,
    pub pending_prefix: Option<Chord>,
    pub confirm_discard: Option<String>,

    pub seq: Option<SeqStatus>,
    pub stashes: Vec<repo::StashEntry>,
    pub ref_prompt: Option<RefPrompt>,
    pub name_input: String,
    pub confirm_delete: Option<DeleteTarget>,
    pub confirm_reset: Option<(Oid, repo::ResetMode)>,

    pub config: Config,
    pub settings_open: bool,

    watch_root: PathBuf,
    watcher: Option<crate::watch::WorktreeWatcher>,
    watcher_started: bool,

    repaint_gate: Arc<AtomicBool>,
    was_hidden: bool,
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
            selected_commit_file: None,
            diff: empty_diff(),
            diff_cursor: 0,
            diff_anchor: None,
            diff_scroll_pending: false,
            diff_scrolled_prev: false,
            diff_visible: None,
            commit_msg: String::new(),
            active_tab: Tab::Graph,
            term: None,
            nvim_socket: std::env::temp_dir()
                .join(format!("twig-nvim-{}.sock", std::process::id())),
            pending_open: None,
            shell: None,
            shell_open: true,
            focus: Pane::Changes,
            changes_cursor: 0,
            sidebar_cursor: 0,
            keymap,
            pending_prefix: None,
            confirm_discard: None,
            seq: None,
            stashes: Vec::new(),
            ref_prompt: None,
            name_input: String::new(),
            confirm_delete: None,
            confirm_reset: None,
            config,
            settings_open: false,
            watch_root: path.clone(),
            watcher: None,
            watcher_started: false,
            repaint_gate: Arc::new(AtomicBool::new(true)),
            was_hidden: false,
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
        self.selected_file = None;
        self.clear_commit_selection();
        self.diff = empty_diff();
        self.reload();
    }

    fn clear_commit_selection(&mut self) {
        self.selected_commit = None;
        self.commit_files.clear();
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
        self.sync_seq_state();
        self.stashes = repo::stash_list(&self.selected);
    }

    fn load_file_diff(&mut self, file: String, staged: bool) {
        let mode = if staged {
            DiffMode::Staged
        } else {
            DiffMode::Unstaged
        };
        match repo::file_diff(&self.selected, &file, mode) {
            Ok(d) => self.diff = d,
            Err(e) => {
                self.diff = FileDiff {
                    rows: Vec::new(),
                    note: Some(format!("diff failed: {e}")),
                    conflict: false,
                }
            }
        }
        self.selected_file = Some((file, staged));
        self.clear_commit_selection();

        self.clamp_diff_nav();
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
    }

    pub fn select_commit(&mut self, oid: Oid) {
        if self
            .selected_commit
            .as_ref()
            .is_some_and(|(o, _)| *o == oid)
        {
            self.clear_commit_selection();
            self.diff = empty_diff();
            return;
        }
        let short = oid.to_string();
        let label = short[..7.min(short.len())].to_string();
        self.commit_files = repo::commit_files(&self.selected, oid).unwrap_or_default();
        self.selected_file = None;
        self.selected_commit = Some((oid, label));
        self.selected_commit_file = None;
        self.diff = FileDiff {
            rows: Vec::new(),
            note: Some("Select a file from the commit".to_string()),
            conflict: false,
        };
    }

    pub fn select_commit_file(&mut self, file: String) {
        let Some((oid, _)) = self.selected_commit else {
            return;
        };
        match repo::commit_file_diff(&self.selected, oid, &file) {
            Ok(d) => self.diff = d,
            Err(e) => {
                self.diff = FileDiff {
                    rows: Vec::new(),
                    note: Some(format!("commit file diff failed: {e}")),
                    conflict: false,
                }
            }
        }
        self.selected_file = None;
        self.selected_commit_file = Some(file);
        self.reset_diff_nav();
        self.active_tab = Tab::Diff;
    }

    pub fn toggle_hunk(&mut self, hunk_index: usize) {
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

    pub fn discard_changes(&mut self, path: &str) {
        if let Err(e) = repo::discard(&self.selected, &[path.to_string()]) {
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
        match repo::commit(&self.selected, self.commit_msg.trim()) {
            Ok(()) => {
                self.commit_msg.clear();
                self.selected_file = None;
                self.clear_commit_selection();
                self.diff = empty_diff();
                self.reload();
            }
            Err(e) => self.error = Some(format!("commit failed: {e}")),
        }
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
            SeqKind::CherryPick => repo::cherry_pick_continue(&self.selected),
            SeqKind::Revert => repo::revert_continue(&self.selected),
        };
        self.apply_seq_outcome(kind, r, "continue");
    }

    pub fn seq_abort(&mut self) {
        let Some(kind) = self.seq.as_ref().map(|s| s.kind) else {
            return;
        };
        let r = match kind {
            SeqKind::Rebase => repo::rebase_abort(&self.selected),
            SeqKind::CherryPick => repo::cherry_pick_abort(&self.selected),
            SeqKind::Revert => repo::revert_abort(&self.selected),
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
        self.ref_prompt = Some(RefPrompt::CreateBranch { at });
    }

    pub fn begin_rename_branch(&mut self, from: String) {
        self.name_input = from.clone();
        self.ref_prompt = Some(RefPrompt::RenameBranch { from });
    }

    pub fn begin_create_tag(&mut self, at: Oid) {
        self.name_input = String::new();
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
        };
        if let Err(e) = res {
            self.error = Some(format!("delete failed: {e}"));
        }
        self.reload();
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
            repo::SeqState::CherryPick => SeqKind::CherryPick,
            repo::SeqState::Revert => SeqKind::Revert,
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
    }
}

fn empty_diff() -> FileDiff {
    FileDiff {
        rows: Vec::new(),
        note: None,
        conflict: false,
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::ui::draw(self, ui);
    }
}
