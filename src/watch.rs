use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct WorktreeWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
    dirty: Arc<AtomicBool>,
}

impl WorktreeWatcher {
    pub fn new(root: &Path, ctx: &egui::Context) -> Result<Self, String> {
        let dirty = Arc::new(AtomicBool::new(false));
        let dirty_cb = dirty.clone();
        let ctx = ctx.clone();

        let mut debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            if events.iter().any(|e| !is_ignored(&e.path)) {
                dirty_cb.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        })
        .map_err(|e| format!("Failed to initialize file watcher: {e}"))?;

        debouncer
            .watcher()
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to start file watcher: {e}"))?;

        Ok(Self {
            _debouncer: debouncer,
            dirty,
        })
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }
}

fn is_ignored(path: &Path) -> bool {
    if path.components().any(|c| c.as_os_str() == ".git") {
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
