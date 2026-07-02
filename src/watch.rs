use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify_debouncer_mini::notify::event::{EventKind, ModifyKind};
use notify_debouncer_mini::notify::{self, Event, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct WorktreeWatcher {
    _watcher: RecommendedWatcher,
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

        watch_worktree(&mut watcher, root)?;

        Ok(Self {
            _watcher: watcher,
            dirty,
        })
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
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

fn watch_worktree(watcher: &mut dyn Watcher, root: &Path) -> Result<(), String> {
    watcher
        .watch(root, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to start file watcher: {e}"))?;

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
    }
    Ok(())
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
