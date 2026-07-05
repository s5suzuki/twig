use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use twig_core::keymap::{Key, KeySource, Modifiers};

pub fn normalize(ev: &KeyEvent) -> Option<(Modifiers, Key)> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let mut mods = Modifiers {
        alt: ev.modifiers.contains(KeyModifiers::ALT),
        ctrl: ev.modifiers.contains(KeyModifiers::CONTROL),
        shift: ev.modifiers.contains(KeyModifiers::SHIFT),
        command: ev.modifiers.contains(KeyModifiers::SUPER),
    };
    let key = match ev.code {
        KeyCode::Char(c) => {
            if c.is_ascii_uppercase() {
                mods.shift = true;
            }
            let mut buf = [0u8; 4];
            Key::from_name(c.encode_utf8(&mut buf))?
        }
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => {
            mods.shift = true;
            Key::Tab
        }
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Up => Key::ArrowUp,
        KeyCode::Down => Key::ArrowDown,
        KeyCode::Left => Key::ArrowLeft,
        KeyCode::Right => Key::ArrowRight,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Insert => Key::Insert,
        KeyCode::Delete => Key::Delete,
        KeyCode::F(n) => match n {
            1 => Key::F1,
            2 => Key::F2,
            3 => Key::F3,
            4 => Key::F4,
            5 => Key::F5,
            6 => Key::F6,
            7 => Key::F7,
            8 => Key::F8,
            9 => Key::F9,
            10 => Key::F10,
            11 => Key::F11,
            12 => Key::F12,
            _ => return None,
        },
        _ => return None,
    };
    Some((mods, key))
}

pub struct KeyQueue(pub Vec<(Modifiers, Key)>);

impl KeyQueue {
    pub fn take(&mut self, mods: Modifiers, key: Key) -> bool {
        self.consume(mods, key)
    }
}

impl KeySource for KeyQueue {
    fn consume(&mut self, mods: Modifiers, key: Key) -> bool {
        if let Some(i) = self.0.iter().position(|&(m, k)| m == mods && k == key) {
            self.0.remove(i);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn chars_map_with_case_and_shift() {
        let (m, k) = normalize(&press(KeyCode::Char('j'), KeyModifiers::NONE)).unwrap();
        assert_eq!(k, Key::J);
        assert!(!m.shift);

        let (m, k) = normalize(&press(KeyCode::Char('G'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(k, Key::G);
        assert!(m.shift);

        let (m, k) = normalize(&press(KeyCode::Char('d'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(k, Key::D);
        assert!(m.ctrl);
    }

    #[test]
    fn punctuation_and_named_keys_map() {
        let (_, k) = normalize(&press(KeyCode::Char('['), KeyModifiers::NONE)).unwrap();
        assert_eq!(k, Key::OpenBracket);
        let (_, k) = normalize(&press(KeyCode::Char('/'), KeyModifiers::NONE)).unwrap();
        assert_eq!(k, Key::Slash);
        let (_, k) = normalize(&press(KeyCode::Char(' '), KeyModifiers::NONE)).unwrap();
        assert_eq!(k, Key::Space);
        let (_, k) = normalize(&press(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
        assert_eq!(k, Key::Enter);

        let (m, k) = normalize(&press(KeyCode::BackTab, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(k, Key::Tab);
        assert!(m.shift);
    }

    #[test]
    fn release_events_are_dropped() {
        let mut ev = press(KeyCode::Char('j'), KeyModifiers::NONE);
        ev.kind = KeyEventKind::Release;
        assert!(normalize(&ev).is_none());
    }
}
