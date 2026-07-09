use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

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

    pub fn wants_mouse_capture(&self) -> bool {
        !self.help_open
            && !self.settings_open
            && self.prompt.is_none()
            && self.active_tab == Tab::Editor
            && self.focus == Pane::RightTab
            && matches!(self.view_mode, ViewMode::All | ViewMode::Single(View::Main))
            && self.term.is_some()
    }

    pub fn handle_mouse(&mut self, events: Vec<MouseEvent>) -> bool {
        if !self.wants_mouse_capture() {
            return false;
        }
        let Some(area) = self.editor_area else {
            return false;
        };
        let flags = self.term.as_ref().unwrap().mouse_flags();

        let mut bytes: Vec<u8> = Vec::new();
        let mut scroll = 0i32;
        for ev in events {
            if ev.column < area.x
                || ev.column >= area.x + area.width
                || ev.row < area.y
                || ev.row >= area.y + area.height
            {
                continue;
            }
            let col = (ev.column - area.x) as usize + 1;
            let row = (ev.row - area.y) as usize + 1;
            let mods = mouse_mods(ev.modifiers);
            match ev.kind {
                MouseEventKind::Down(btn) => {
                    if !flags.report {
                        continue;
                    }
                    let base = btn_base(btn);
                    self.mouse_pressed = Some(base);
                    self.last_mouse_cell = Some((ev.column, ev.row));
                    bytes.extend_from_slice(&sgr_mouse(base + mods, col, row, true));
                }
                MouseEventKind::Up(btn) => {
                    if !flags.report {
                        continue;
                    }
                    self.mouse_pressed = None;
                    bytes.extend_from_slice(&sgr_mouse(btn_base(btn) + mods, col, row, false));
                }
                MouseEventKind::Drag(btn) => {
                    if !flags.report || !(flags.motion || flags.drag) {
                        continue;
                    }
                    if self.last_mouse_cell == Some((ev.column, ev.row)) {
                        continue;
                    }
                    self.last_mouse_cell = Some((ev.column, ev.row));
                    bytes.extend_from_slice(&sgr_mouse(btn_base(btn) + 32 + mods, col, row, true));
                }
                MouseEventKind::Moved => {
                    if !flags.report || !flags.motion {
                        continue;
                    }
                    if self.last_mouse_cell == Some((ev.column, ev.row)) {
                        continue;
                    }
                    self.last_mouse_cell = Some((ev.column, ev.row));
                    bytes.extend_from_slice(&sgr_mouse(3 + 32 + mods, col, row, true));
                }
                MouseEventKind::ScrollUp => {
                    if flags.report {
                        bytes.extend_from_slice(&sgr_mouse(64 + mods, col, row, true));
                    } else {
                        scroll += 1;
                    }
                }
                MouseEventKind::ScrollDown => {
                    if flags.report {
                        bytes.extend_from_slice(&sgr_mouse(65 + mods, col, row, true));
                    } else {
                        scroll -= 1;
                    }
                }
                _ => {}
            }
        }

        let mut dirty = false;
        if let Some(t) = self.term.as_mut() {
            if !bytes.is_empty() {
                t.feed(&bytes);
                dirty = true;
            }
            if scroll != 0 {
                t.scroll_lines(scroll);
                dirty = true;
            }
        }
        dirty
    }

    pub(super) fn handle_editor_key(&mut self, ev: KeyEvent) {
        if ev.kind == KeyEventKind::Release {
            return;
        }
        let alt = ev.modifiers.contains(KeyModifiers::ALT);
        match ev.code {
            KeyCode::Char('h') if alt => return self.focus_move(-1),
            KeyCode::Char('l') if alt => return self.focus_move(1),
            _ => {}
        }
        if let Some(step) = cycle_step(
            &self.keymap,
            &mut self.pending_prefix,
            self.kb_enhanced,
            &ev,
        ) {
            return self.cycle_tab(step);
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

fn btn_base(btn: MouseButton) -> u8 {
    match btn {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

fn mouse_mods(m: KeyModifiers) -> u8 {
    let mut b = 0;
    if m.contains(KeyModifiers::SHIFT) {
        b += 4;
    }
    if m.contains(KeyModifiers::ALT) {
        b += 8;
    }
    if m.contains(KeyModifiers::CONTROL) {
        b += 16;
    }
    b
}

pub(super) fn cycle_step(
    keymap: &Keymap,
    pending: &mut Option<Chord>,
    kb_enhanced: bool,
    ev: &KeyEvent,
) -> Option<isize> {
    let escapes = KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER;
    if kb_enhanced && !ev.modifiers.intersects(escapes) {
        return None;
    }
    let mut queue = KeyQueue(crate::keys::normalize(ev).into_iter().collect());
    let actions = keymap.resolve(&mut queue, Context::Global, pending, |a| {
        matches!(
            a,
            Action::CycleTab | Action::CycleTabFwd | Action::CycleTabBack
        )
    });
    match actions.first()? {
        Action::CycleTabBack => Some(-1),
        _ => Some(1),
    }
}

fn sgr_mouse(button: u8, col: usize, row: usize, pressed: bool) -> Vec<u8> {
    let f = if pressed { 'M' } else { 'm' };
    format!("\x1b[<{button};{col};{row}{f}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_encodes_press_and_release() {
        assert_eq!(sgr_mouse(0, 1, 3, true), b"\x1b[<0;1;3M".to_vec());
        assert_eq!(sgr_mouse(0, 1, 3, false), b"\x1b[<0;1;3m".to_vec());
        assert_eq!(sgr_mouse(64, 5, 9, true), b"\x1b[<64;5;9M".to_vec());
    }

    fn step(kb_enhanced: bool, code: KeyCode, mods: KeyModifiers) -> Option<isize> {
        let keymap = Keymap::default();
        let mut pending = None;
        cycle_step(
            &keymap,
            &mut pending,
            kb_enhanced,
            &KeyEvent::new(code, mods),
        )
    }

    #[test]
    fn kitty_terminal_passes_tab_to_nvim_and_cycles_on_ctrl_tab() {
        const CTRL: KeyModifiers = KeyModifiers::CONTROL;
        const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
        assert_eq!(step(true, KeyCode::Tab, KeyModifiers::NONE), None);
        assert_eq!(step(true, KeyCode::Tab, SHIFT), None);
        assert_eq!(step(true, KeyCode::BackTab, SHIFT), None);
        assert_eq!(step(true, KeyCode::Tab, CTRL), Some(1));
        assert_eq!(step(true, KeyCode::Tab, CTRL | SHIFT), Some(-1));
    }

    #[test]
    fn plain_terminal_keeps_tab_as_the_escape_hatch() {
        assert_eq!(step(false, KeyCode::Tab, KeyModifiers::NONE), Some(1));
        assert_eq!(step(false, KeyCode::BackTab, KeyModifiers::SHIFT), Some(-1));
    }

    #[test]
    fn nvim_keys_are_never_swallowed_as_tab_cycles() {
        for kb in [true, false] {
            for code in [KeyCode::Char('o'), KeyCode::Char('i'), KeyCode::Char('w')] {
                assert_eq!(step(kb, code, KeyModifiers::CONTROL), None);
            }
            assert_eq!(step(kb, KeyCode::Char('a'), KeyModifiers::NONE), None);
            assert_eq!(step(kb, KeyCode::Esc, KeyModifiers::NONE), None);
        }
    }

    #[test]
    fn button_and_modifier_codes_match_xterm() {
        assert_eq!(btn_base(MouseButton::Left), 0);
        assert_eq!(btn_base(MouseButton::Middle), 1);
        assert_eq!(btn_base(MouseButton::Right), 2);
        assert_eq!(mouse_mods(KeyModifiers::NONE), 0);
        assert_eq!(mouse_mods(KeyModifiers::SHIFT), 4);
        assert_eq!(mouse_mods(KeyModifiers::ALT), 8);
        assert_eq!(mouse_mods(KeyModifiers::CONTROL), 16);
        assert_eq!(mouse_mods(KeyModifiers::SHIFT | KeyModifiers::CONTROL), 20);
    }
}
