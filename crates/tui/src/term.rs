use std::path::Path;
use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use twit_term::alacritty_terminal::term::cell::Flags;
use twit_term::alacritty_terminal::vte::ansi::CursorShape;
use twit_term::{TermBackend, color_rgb};

pub struct EditorTerm {
    pub be: TermBackend,
}

impl EditorTerm {
    pub fn spawn_nvim(socket: &Path, cwd: &Path) -> Result<Self, String> {
        if let Some(dir) = socket.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        TermBackend::spawn_nvim(socket, cwd, Arc::new(|| {})).map(|be| Self { be })
    }

    pub fn pump(&mut self) -> bool {
        self.be.pump()
    }

    pub fn is_alive(&mut self) -> bool {
        self.be.is_alive()
    }

    pub fn feed_key(&mut self, ev: &KeyEvent) {
        if let Some(bytes) = key_to_bytes(ev) {
            self.be.feed(&bytes);
        }
    }

    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, focused: bool) {
        let cols = (area.width as usize).max(1);
        let rows = (area.height as usize).max(1);
        if cols != self.be.cols() || rows != self.be.rows() {
            self.be.resize(cols, rows);
        }

        let content = self.be.term.renderable_content();
        let off = content.display_offset as i32;
        for ind in content.display_iter {
            let row = ind.point.line.0 + off;
            let col = ind.point.column.0;
            if row < 0 || row >= rows as i32 || col >= cols {
                continue;
            }
            let cell = ind.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let x = area.x + col as u16;
            let y = area.y + row as u16;
            let Some(slot) = buf.cell_mut((x, y)) else {
                continue;
            };
            let c = if cell.c == '\0' { ' ' } else { cell.c };
            slot.set_char(c);
            let mut style = Style::default();
            if let Some((r, g, b)) = color_rgb(cell.fg) {
                style = style.fg(Color::Rgb(r, g, b));
            }
            if let Some((r, g, b)) = color_rgb(cell.bg) {
                style = style.bg(Color::Rgb(r, g, b));
            }
            if cell.flags.contains(Flags::INVERSE) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if cell.flags.contains(Flags::BOLD) {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.flags.contains(Flags::ITALIC) {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.flags.contains(Flags::UNDERLINE) {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            slot.set_style(style);
        }

        let cur = content.cursor;
        let cursor_row = cur.point.line.0 + off;
        if focused
            && cur.shape != CursorShape::Hidden
            && cursor_row >= 0
            && (cursor_row as usize) < rows
            && cur.point.column.0 < cols
        {
            let x = area.x + cur.point.column.0 as u16;
            let y = area.y + cursor_row as u16;
            if let Some(slot) = buf.cell_mut((x, y)) {
                let style = slot.style().add_modifier(Modifier::REVERSED);
                slot.set_style(style);
            }
        }
    }
}

pub fn key_to_bytes(ev: &KeyEvent) -> Option<Vec<u8>> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    let alt = ev.modifiers.contains(KeyModifiers::ALT);
    let mut out: Vec<u8> = Vec::new();
    if alt {
        out.push(0x1b);
    }
    match ev.code {
        KeyCode::Char(c) => {
            if ctrl {
                let b = match c.to_ascii_lowercase() {
                    l @ 'a'..='z' => l as u8 - b'a' + 1,
                    ' ' | '@' => 0,
                    '[' => 27,
                    '\\' => 28,
                    ']' => 29,
                    '^' => 30,
                    '_' | '/' => 31,
                    _ => return None,
                };
                out.push(b);
            } else {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        KeyCode::Enter => out.push(b'\r'),
        KeyCode::Backspace => out.push(0x7f),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Up => out.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => out.extend_from_slice(b"\x1b[B"),
        KeyCode::Right => out.extend_from_slice(b"\x1b[C"),
        KeyCode::Left => out.extend_from_slice(b"\x1b[D"),
        KeyCode::Home => out.extend_from_slice(b"\x1b[H"),
        KeyCode::End => out.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => out.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => out.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => out.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => out.extend_from_slice(b"\x1b[2~"),
        KeyCode::F(n) => match n {
            1 => out.extend_from_slice(b"\x1bOP"),
            2 => out.extend_from_slice(b"\x1bOQ"),
            3 => out.extend_from_slice(b"\x1bOR"),
            4 => out.extend_from_slice(b"\x1bOS"),
            5 => out.extend_from_slice(b"\x1b[15~"),
            6 => out.extend_from_slice(b"\x1b[17~"),
            7 => out.extend_from_slice(b"\x1b[18~"),
            8 => out.extend_from_slice(b"\x1b[19~"),
            9 => out.extend_from_slice(b"\x1b[20~"),
            10 => out.extend_from_slice(b"\x1b[21~"),
            11 => out.extend_from_slice(b"\x1b[23~"),
            12 => out.extend_from_slice(b"\x1b[24~"),
            _ => return None,
        },
        _ => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn key_to_bytes_maps_text_ctrl_and_escapes() {
        assert_eq!(
            key_to_bytes(&press(KeyCode::Char('a'), KeyModifiers::NONE)),
            Some(b"a".to_vec())
        );
        assert_eq!(
            key_to_bytes(&press(KeyCode::Char('あ'), KeyModifiers::NONE)),
            Some("あ".as_bytes().to_vec())
        );
        assert_eq!(
            key_to_bytes(&press(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(vec![0x03])
        );
        assert_eq!(
            key_to_bytes(&press(KeyCode::Char('x'), KeyModifiers::ALT)),
            Some(vec![0x1b, b'x'])
        );
        assert_eq!(
            key_to_bytes(&press(KeyCode::Esc, KeyModifiers::NONE)),
            Some(vec![0x1b])
        );
        assert_eq!(
            key_to_bytes(&press(KeyCode::Up, KeyModifiers::NONE)),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_to_bytes(&press(KeyCode::Tab, KeyModifiers::NONE)),
            Some(b"\t".to_vec())
        );
        let mut rel = press(KeyCode::Char('a'), KeyModifiers::NONE);
        rel.kind = KeyEventKind::Release;
        assert_eq!(key_to_bytes(&rel), None);
    }
}
