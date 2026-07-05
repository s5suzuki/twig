use std::path::{Path, PathBuf};

use twig_core::config::Config;
use twig_core::keymap::{Action, Chord, Context, Key, Keymap, Modifiers};
use twig_core::repo::{self, RepoNode, StatusEntry};

use crate::keys::KeyQueue;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sidebar,
    Changes,
    RightTab,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Graph,
    Diff,
}

pub struct TuiApp {
    pub root: RepoNode,
    pub selected: PathBuf,
    pub staged: Vec<StatusEntry>,
    pub unstaged: Vec<StatusEntry>,
    pub focus: Pane,
    pub active_tab: Tab,
    pub keymap: Keymap,
    pub pending_prefix: Option<Chord>,
    pub error: Option<String>,
    pub quit: bool,
}

impl TuiApp {
    pub fn new(path: &Path) -> Result<Self, String> {
        let config = Config::load();
        let root = repo::discover(path).map_err(|e| e.to_string())?;
        let (staged, unstaged) = repo::load_status(path).map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            selected: path.to_path_buf(),
            staged,
            unstaged,
            focus: Pane::Changes,
            active_tab: Tab::Graph,
            keymap: Keymap::from_config(&config.keys),
            pending_prefix: None,
            error: None,
            quit: false,
        })
    }

    pub fn refresh(&mut self) {
        repo::refresh_badges(&mut self.root);
        match repo::load_status(&self.selected) {
            Ok((staged, unstaged)) => {
                self.staged = staged;
                self.unstaged = unstaged;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("status failed: {e}")),
        }
    }

    pub fn handle_keys(&mut self, mut keys: KeyQueue) {
        if keys.take(Modifiers::NONE, Key::Q) || keys.take(Modifiers::CTRL, Key::C) {
            self.quit = true;
            return;
        }

        let actions = self
            .keymap
            .resolve(&mut keys, Context::Global, &mut self.pending_prefix, |a| {
                matches!(
                    a,
                    Action::FocusLeft
                        | Action::FocusRight
                        | Action::CycleTab
                        | Action::CycleTabFwd
                        | Action::CycleTabBack
                )
            });
        for a in actions {
            match a {
                Action::FocusLeft => self.focus_move(-1),
                Action::FocusRight => self.focus_move(1),
                Action::CycleTab | Action::CycleTabFwd => self.cycle_tab(1),
                Action::CycleTabBack => self.cycle_tab(-1),
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
