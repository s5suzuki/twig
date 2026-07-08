use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Tab;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SharedState {
    pub generation: u64,
    pub selected_repo: PathBuf,
    pub selected_file: Option<(String, bool)>,
    pub selected_commit: Option<String>,
    #[serde(default)]
    pub selected_commit_file: Option<String>,
    pub active_tab: Tab,
    pub quit: bool,
    pub panes: BTreeMap<String, u32>,
    #[serde(default)]
    pub zellij_panes: BTreeMap<String, String>,
    #[serde(default)]
    pub extra_panes: Vec<String>,
    #[serde(default)]
    pub editor_file: Option<String>,
    #[serde(default)]
    pub editor_line: Option<u32>,
    #[serde(default)]
    pub editor_seq: u64,
}

impl SharedState {
    fn new(repo: &Path) -> Self {
        Self {
            generation: 0,
            selected_repo: repo.to_path_buf(),
            selected_file: None,
            selected_commit: None,
            selected_commit_file: None,
            active_tab: Tab::Graph,
            quit: false,
            panes: BTreeMap::new(),
            zellij_panes: BTreeMap::new(),
            extra_panes: Vec::new(),
            editor_file: None,
            editor_line: None,
            editor_seq: 0,
        }
    }
}

pub fn register_extra_pane(dir: &Path, repo: &Path, pane_id: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("session dir: {e}"))?;
    let lock = lock_dir(dir);
    let path = dir.join("state.json");
    let mut state = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<SharedState>(&b).ok())
        .filter(|s| !s.quit)
        .unwrap_or_else(|| SharedState::new(repo));
    if !state.extra_panes.iter().any(|p| p == pane_id) {
        state.extra_panes.push(pane_id.to_string());
    }
    state.generation += 1;
    let tmp = dir.join(format!(".state.{}.tmp", std::process::id()));
    let json = serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    let written = std::fs::rename(&tmp, &path).map_err(|e| e.to_string());
    drop(lock);
    written
}

pub fn state_gone_or_quit(dir: &Path) -> bool {
    match std::fs::read(dir.join("state.json")) {
        Err(_) => true,
        Ok(bytes) => serde_json::from_slice::<SharedState>(&bytes)
            .map(|s| s.quit)
            .unwrap_or(false),
    }
}

fn lock_dir(dir: &Path) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(dir.join("lock"))
        .ok()?;
    file.lock().ok()?;
    Some(file)
}

pub fn session_dir(token: &str) -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("twig")
        .join(token)
}

pub fn repo_token(repo: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    repo.hash(&mut h);
    format!("r{:016x}", h.finish())
}

pub fn pid_token() -> String {
    format!("p{}", std::process::id())
}

pub fn pid_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

pub struct Tick {
    pub changed: Option<SharedState>,
    pub quit: bool,
}

pub struct Session {
    dir: PathBuf,
    view: String,
    pid: u32,
    repo: PathBuf,
    zellij_pane: Option<String>,
    last_gen: u64,
}

impl Session {
    pub fn join(
        dir: &Path,
        view: &str,
        pid: u32,
        repo: &Path,
        zellij_pane: Option<String>,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("session dir: {e}"))?;
        let sess = Self {
            dir: dir.to_path_buf(),
            view: view.to_string(),
            pid,
            repo: repo.to_path_buf(),
            zellij_pane,
            last_gen: 0,
        };
        let lock = sess.lock();
        let mut state = sess
            .read()
            .filter(|s| !s.quit)
            .unwrap_or_else(|| SharedState::new(repo));
        state.panes.retain(|_, p| pid_alive(*p));
        state.panes.insert(view.to_string(), pid);
        match &sess.zellij_pane {
            Some(id) => {
                state.zellij_panes.insert(view.to_string(), id.clone());
            }
            None => {
                state.zellij_panes.remove(view);
            }
        }
        state
            .zellij_panes
            .retain(|v, _| state.panes.contains_key(v));
        state.generation += 1;
        let written = sess.write(&state);
        drop(lock);
        written?;
        Ok(sess)
    }

    pub fn request_editor(&mut self, file: &Path, line: Option<u32>) -> bool {
        let has_main = self.read().is_some_and(|s| s.panes.contains_key("main"));
        if !has_main {
            return false;
        }
        let file = file.to_string_lossy().into_owned();
        self.publish(|st| {
            st.editor_file = Some(file);
            st.editor_line = line;
            st.editor_seq += 1;
        });
        true
    }

    pub fn editor_target_pane(&self) -> Option<String> {
        let state = self.read()?;
        let target = state.zellij_panes.get("main")?;
        if Some(target) == self.zellij_pane.as_ref() {
            return None;
        }
        Some(target.clone())
    }

    pub fn diff_target_pane(&self) -> Option<String> {
        let state = self.read()?;
        let target = state
            .zellij_panes
            .get("diff")
            .or_else(|| state.zellij_panes.get("main"))?;
        if Some(target) == self.zellij_pane.as_ref() {
            return None;
        }
        Some(target.clone())
    }

    fn lock(&self) -> Option<std::fs::File> {
        lock_dir(&self.dir)
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn read(&self) -> Option<SharedState> {
        let bytes = std::fs::read(self.state_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write(&self, state: &SharedState) -> Result<(), String> {
        let tmp = self.dir.join(format!(".state.{}.tmp", self.pid));
        let json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.state_path()).map_err(|e| e.to_string())
    }

    pub fn publish(&mut self, f: impl FnOnce(&mut SharedState)) {
        let lock = self.lock();
        let mut state = self.read().unwrap_or_else(|| SharedState::new(&self.repo));
        state.panes.insert(self.view.clone(), self.pid);
        f(&mut state);
        state.generation += 1;
        self.last_gen = state.generation;
        let _ = self.write(&state);
        drop(lock);
    }

    pub fn tick(&mut self) -> Tick {
        let Some(state) = self.read() else {
            return Tick {
                changed: None,
                quit: true,
            };
        };
        let quit = state.quit;
        let changed = if state.generation > self.last_gen {
            self.last_gen = state.generation;
            Some(state)
        } else {
            None
        };
        Tick { changed, quit }
    }

    pub fn shutdown(&mut self, broadcast_quit: bool) -> Vec<String> {
        let lock = self.lock();
        let Some(mut state) = self.read() else {
            let _ = std::fs::remove_dir_all(&self.dir);
            return Vec::new();
        };
        state.panes.remove(&self.view);
        state.panes.retain(|_, p| pid_alive(*p));
        state
            .zellij_panes
            .retain(|v, _| state.panes.contains_key(v));
        if state.panes.is_empty() {
            let _ = std::fs::remove_dir_all(&self.dir);
            return state.extra_panes;
        }
        if broadcast_quit {
            state.quit = true;
        }
        state.generation += 1;
        let _ = self.write(&state);
        drop(lock);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("twig-session-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn join_publish_read_roundtrip_bumps_generation() {
        let dir = temp_dir();
        let repo = Path::new("/repo/a");
        let mut sess = Session::join(&dir, "changes", std::process::id(), repo, None).unwrap();
        let state = sess.read().unwrap();
        assert_eq!(state.selected_repo, repo);
        assert_eq!(state.panes.get("changes"), Some(&std::process::id()));
        let g0 = state.generation;

        sess.publish(|s| s.selected_file = Some(("a.txt".into(), false)));
        let state = sess.read().unwrap();
        assert!(state.generation > g0);
        assert_eq!(state.selected_file, Some(("a.txt".into(), false)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_state_is_discarded_on_join() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("state.json"), b"{not json").unwrap();
        let sess =
            Session::join(&dir, "main", std::process::id(), Path::new("/repo/b"), None).unwrap();
        let state = sess.read().unwrap();
        assert_eq!(state.selected_repo, Path::new("/repo/b"));
        assert!(!state.quit);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_quit_state_is_replaced_on_join() {
        let dir = temp_dir();
        let repo = Path::new("/repo/c");
        let mut sess = Session::join(&dir, "main", std::process::id(), repo, None).unwrap();
        sess.publish(|s| {
            s.quit = true;
            s.selected_file = Some(("old.txt".into(), true));
        });

        let sess2 = Session::join(&dir, "changes", std::process::id(), repo, None).unwrap();
        let state = sess2.read().unwrap();
        assert!(!state.quit);
        assert_eq!(state.selected_file, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tick_reports_change_once_and_detects_quit_broadcast() {
        let dir = temp_dir();
        let repo = Path::new("/repo/d");
        let pid = std::process::id();
        let mut a = Session::join(&dir, "changes", pid, repo, None).unwrap();
        let mut b = Session::join(&dir, "main", pid, repo, None).unwrap();
        let _ = b.tick();

        a.publish(|s| s.selected_commit = Some("abc".into()));
        let t = b.tick();
        assert_eq!(t.changed.unwrap().selected_commit, Some("abc".into()));
        assert!(!t.quit);
        assert!(
            b.tick().changed.is_none(),
            "same generation not re-reported"
        );

        a.publish(|s| s.quit = true);
        assert!(b.tick().quit);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dead_pid_in_panes_does_not_trigger_quit() {
        let dir = temp_dir();
        let repo = Path::new("/repo/e");
        let mut me = Session::join(&dir, "main", std::process::id(), repo, None).unwrap();
        let dead = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = dead.id();
        let sibling = Session::join(&dir, "changes", dead_pid, repo, None).unwrap();
        let _ = sibling;
        let mut child = dead;
        child.wait().unwrap();
        while pid_alive(dead_pid) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !me.tick().quit,
            "a dead sibling pane must not quit the others; pane lifecycle is Zellij's job"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_target_prefers_dedicated_diff_pane_and_skips_self() {
        let dir = temp_dir();
        let repo = Path::new("/repo/g");
        let pid = std::process::id();
        let changes = Session::join(&dir, "changes", pid, repo, Some("1".into())).unwrap();
        let main = Session::join(&dir, "main", pid, repo, Some("2".into())).unwrap();
        assert_eq!(changes.diff_target_pane(), Some("2".to_string()));

        let diff = Session::join(&dir, "diff", pid, repo, Some("3".into())).unwrap();
        assert_eq!(changes.diff_target_pane(), Some("3".to_string()));
        assert_eq!(main.diff_target_pane(), Some("3".to_string()));
        assert_eq!(
            diff.diff_target_pane(),
            None,
            "own pane is not a jump target"
        );

        let outside = Session::join(&dir, "graph", pid, repo, None).unwrap();
        assert_eq!(outside.diff_target_pane(), Some("3".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_gone_or_quit_tracks_session_teardown() {
        let dir = temp_dir();
        let repo = Path::new("/repo/i");
        register_extra_pane(&dir, repo, "terminal_1").unwrap();
        assert!(!state_gone_or_quit(&dir), "live session keeps the shell");

        let mut sess = Session::join(&dir, "main", std::process::id(), repo, None).unwrap();
        sess.publish(|s| s.quit = true);
        assert!(state_gone_or_quit(&dir), "quit broadcast stops the shell");

        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            state_gone_or_quit(&dir),
            "removed session dir stops the shell"
        );
    }

    #[test]
    fn extra_pane_ids_survive_joins_and_return_to_the_last_pane() {
        let dir = temp_dir();
        let repo = Path::new("/repo/h");
        let pid = std::process::id();
        register_extra_pane(&dir, repo, "terminal_9").unwrap();
        register_extra_pane(&dir, repo, "terminal_9").unwrap();

        let mut a = Session::join(&dir, "changes", pid, repo, None).unwrap();
        let mut b = Session::join(&dir, "main", pid, repo, None).unwrap();
        assert_eq!(
            a.read().unwrap().extra_panes,
            vec!["terminal_9".to_string()],
            "registration is idempotent and survives joins"
        );

        assert!(a.shutdown(true).is_empty(), "non-final pane closes nothing");
        assert_eq!(
            b.shutdown(false),
            vec!["terminal_9".to_string()],
            "last pane receives the extra panes to close"
        );
        assert!(!dir.exists());
    }

    #[test]
    fn last_pane_shutdown_removes_session_dir() {
        let dir = temp_dir();
        let repo = Path::new("/repo/f");
        let pid = std::process::id();
        let mut a = Session::join(&dir, "changes", pid, repo, None).unwrap();
        let mut b = Session::join(&dir, "main", pid, repo, None).unwrap();
        a.shutdown(true);
        assert!(
            dir.join("state.json").exists(),
            "state kept while a pane remains"
        );
        assert!(b.read().unwrap().quit, "quit broadcast to remaining panes");
        b.shutdown(false);
        assert!(!dir.exists(), "last pane cleans up the session dir");
    }
}
