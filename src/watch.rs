use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify_debouncer_mini::notify::event::{EventKind, ModifyKind};
use notify_debouncer_mini::notify::{self, Event, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct WorktreeWatcher {
    watcher: RecommendedWatcher,
    watched_top: HashSet<PathBuf>,
    dirty: Arc<AtomicBool>,
}

impl WorktreeWatcher {
    pub fn new(
        root: &Path,
        ctx: &egui::Context,
        repaint_gate: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let dirty = Arc::new(AtomicBool::new(false));
        let gitignore = build_gitignore(root);

        let (tx, rx) = mpsc::channel::<()>();
        spawn_debounce(rx, dirty.clone(), ctx.clone(), repaint_gate);

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            if !is_change(&event.kind) {
                return;
            }
            if event.paths.iter().all(|p| is_ignored(&gitignore, p)) {
                return;
            }
            let _ = tx.send(());
        })
        .map_err(|e| format!("Failed to initialize file watcher: {e}"))?;

        let watched_top = watch_worktree(&mut watcher, root)?;

        Ok(Self {
            watcher,
            watched_top,
            dirty,
        })
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    pub fn rescan_new_toplevel(&mut self, root: &Path) {
        self.watched_top.retain(|path| {
            if path.is_dir() {
                return true;
            }
            let _ = self.watcher.unwatch(path);
            false
        });

        let gitignore = build_gitignore(root);
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if self.watched_top.contains(&path) {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some(".git")
                || is_ignored(&gitignore, &path)
            {
                continue;
            }
            if self.watcher.watch(&path, RecursiveMode::Recursive).is_ok() {
                self.watched_top.insert(path);
            }
        }
    }
}

fn is_change(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(_))
    )
}

fn spawn_debounce(
    rx: mpsc::Receiver<()>,
    dirty: Arc<AtomicBool>,
    ctx: egui::Context,
    repaint_gate: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            while rx.recv_timeout(DEBOUNCE).is_ok() {}
            dirty.store(true, Ordering::Relaxed);
            if repaint_gate.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
        }
    });
}

fn watch_worktree(watcher: &mut dyn Watcher, root: &Path) -> Result<HashSet<PathBuf>, String> {
    watcher
        .watch(root, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to start file watcher: {e}"))?;

    let mut watched_top = HashSet::new();
    let gitignore = build_gitignore(root);
    let entries = std::fs::read_dir(root).map_err(|e| format!("Failed to read repo root: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some(".git")
            || is_ignored(&gitignore, &path)
        {
            continue;
        }
        watcher
            .watch(&path, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch {}: {e}", path.display()))?;
        watched_top.insert(path);
    }
    Ok(watched_top)
}

fn build_gitignore(root: &Path) -> Gitignore {
    let mut b = GitignoreBuilder::new(root);
    let _ = b.add(root.join(".gitignore"));
    let _ = b.add_line(None, ".git/");
    let _ = b.add_line(None, "target/");
    b.build().unwrap_or_else(|_| Gitignore::empty())
}

fn is_ignored(gitignore: &Gitignore, path: &Path) -> bool {
    if path.components().any(|c| c.as_os_str() == ".git") {
        return true;
    }
    if gitignore
        .matched_path_or_any_parents(path, path.is_dir())
        .is_ignore()
    {
        return true;
    }

    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swo")
        || name.ends_with(".swn")
        || name.ends_with(".swx")
        || name.ends_with(".un~")
        || name.ends_with(".tmp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet as StdHashSet;

    fn inotify_inodes() -> StdHashSet<u64> {
        let mut inodes = StdHashSet::new();
        for fd in std::fs::read_dir("/proc/self/fd").unwrap().flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            if !target.to_string_lossy().contains("inotify") {
                continue;
            }
            let name = fd.file_name();
            let info = std::fs::read_to_string(format!(
                "/proc/self/fdinfo/{}",
                name.to_string_lossy()
            ))
            .unwrap_or_default();
            for line in info.lines() {
                if let Some(rest) = line.strip_prefix("inotify ") {
                    for tok in rest.split_whitespace() {
                        if let Some(hex) = tok.strip_prefix("ino:") {
                            if let Ok(ino) = u64::from_str_radix(hex, 16) {
                                inodes.insert(ino);
                            }
                        }
                    }
                }
            }
        }
        inodes
    }

    fn ino(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).unwrap().ino()
    }

    #[test]
    fn rescan_adds_new_toplevel_but_skips_ignored_and_git() {
        let tmp = std::env::temp_dir().join(format!("twig-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        std::fs::write(tmp.join(".gitignore"), "node_modules/\n").unwrap();
        std::fs::create_dir(tmp.join("existing")).unwrap();

        let ctx = egui::Context::default();
        let mut w =
            WorktreeWatcher::new(&tmp, &ctx, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(w.watched_top.contains(&tmp.join("existing")));

        std::fs::create_dir(tmp.join("newdir")).unwrap();
        std::fs::create_dir(tmp.join("node_modules")).unwrap();
        w.rescan_new_toplevel(&tmp);

        let watched = inotify_inodes();
        assert!(w.watched_top.contains(&tmp.join("newdir")));
        assert!(watched.contains(&ino(&tmp.join("newdir"))), "newdir must be inotify-watched");
        assert!(!w.watched_top.contains(&tmp.join("node_modules")), "gitignored dir must be skipped");
        assert!(!w.watched_top.iter().any(|p| p.ends_with(".git")), ".git must be skipped");

        std::fs::remove_dir_all(tmp.join("newdir")).unwrap();
        w.rescan_new_toplevel(&tmp);
        assert!(!w.watched_top.contains(&tmp.join("newdir")), "deleted dir must be pruned");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
