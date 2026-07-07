use super::*;

pub(super) fn nvim_socket_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("twig")
        .join(format!("{}-{n}-nvim.sock", std::process::id()))
}

impl TuiApp {
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

    pub(super) fn editor_focused(&mut self) -> bool {
        self.active_tab == Tab::Editor
            && self.focus == Pane::RightTab
            && matches!(self.view_mode, ViewMode::All | ViewMode::Single(View::Main))
            && self.term.as_mut().is_some_and(|t| t.is_alive())
    }

    pub(super) fn handle_editor_key(&mut self, ev: KeyEvent) {
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

    pub fn open_in_embedded(&mut self, file: &Path, line: Option<u32>) -> bool {
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
            match twit_core::editor::open_abs_in_server_at(file, &self.nvim_socket, line) {
                Ok(()) => self.error = None,
                Err(e) => self.error = Some(e),
            }
        } else {
            self.pending_open = Some((
                file.to_path_buf(),
                line,
                std::time::Instant::now() + std::time::Duration::from_secs(10),
            ));
        }
        true
    }

    pub fn poll_pending_open(&mut self) -> bool {
        let Some((file, line, deadline)) = self.pending_open.clone() else {
            return false;
        };
        if !self.term.as_mut().is_some_and(|t| t.is_alive()) {
            self.pending_open = None;
            return true;
        }
        if self.nvim_socket.exists() {
            self.pending_open = None;
            match twit_core::editor::open_abs_in_server_at(&file, &self.nvim_socket, line) {
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
