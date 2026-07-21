use std::path::Path;

use crate::{herdr, zellij};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mux {
    Herdr,
    Zellij,
}

pub struct SplitPlan {
    pub self_resize_cols: Option<u16>,
}

pub fn active() -> Option<Mux> {
    if herdr::inside_herdr() {
        Some(Mux::Herdr)
    } else if zellij::inside_zellij() {
        Some(Mux::Zellij)
    } else {
        None
    }
}

pub fn current_pane_id() -> Option<String> {
    match active() {
        Some(Mux::Herdr) => herdr::current_pane_id(),
        Some(Mux::Zellij) => std::env::var("ZELLIJ_PANE_ID")
            .ok()
            .filter(|v| !v.is_empty()),
        None => None,
    }
}

impl Mux {
    pub fn split_current_tab(self, repo: &Path, token: &str) -> Result<SplitPlan, String> {
        match self {
            Mux::Herdr => herdr::split_current_tab(repo, token).map(|()| SplitPlan {
                self_resize_cols: None,
            }),
            Mux::Zellij => zellij::split_current_tab(repo, token).map(|()| SplitPlan {
                self_resize_cols: Some(26),
            }),
        }
    }

    pub fn spawn_tab(self, repo: &Path) -> Result<(), String> {
        match self {
            Mux::Herdr => herdr::spawn_tab(repo),
            Mux::Zellij => zellij::spawn_tab(repo),
        }
    }
}

pub fn focus_pane(target: &str) {
    match active() {
        Some(Mux::Herdr) => herdr::focus_pane(target),
        Some(Mux::Zellij) => zellij::focus_pane(target),
        None => {}
    }
}

pub fn close_pane(id: &str) {
    match active() {
        Some(Mux::Herdr) => herdr::close_pane(id),
        Some(Mux::Zellij) => zellij::close_pane(id),
        None => {}
    }
}

pub fn resize_self_step(direction: &str) {
    if let Some(Mux::Zellij) = active() {
        zellij::resize_self_step(direction);
    }
}
