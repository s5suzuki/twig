use super::*;

pub(super) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl App {
    pub fn open_in_editor(&mut self, file: &str) {
        let abs = self.selected.join(file);
        self.open_abs_in_editor(abs, None);
    }

    pub fn open_in_editor_at(&mut self, file: &str, line: u32) {
        let abs = self.selected.join(file);
        self.open_abs_in_editor(abs, Some(line));
    }

    pub fn open_abs_in_editor(&mut self, abs: PathBuf, line: Option<u32>) {
        self.active_tab = Tab::Editor;
        self.focus = Pane::RightTab;
        if self.term.is_some() && self.nvim_socket.exists() {
            if let Err(e) = twit_core::editor::open_abs_in_server_at(&abs, &self.nvim_socket, line)
            {
                self.error = Some(e);
            }
        } else {
            self.pending_open = Some((abs, line));
        }
    }

    pub fn flush_pending_open(&mut self) -> bool {
        let Some((abs, line)) = self.pending_open.clone() else {
            return false;
        };
        if self.term.is_none() || !self.nvim_socket.exists() {
            return true;
        }
        if let Err(e) = twit_core::editor::open_abs_in_server_at(&abs, &self.nvim_socket, line) {
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
}
