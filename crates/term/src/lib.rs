use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

pub use alacritty_terminal;

use alacritty_terminal::Term as AlacTerm;
use alacritty_terminal::event::{Event as AlacEvent, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub type Notifier = Arc<dyn Fn() + Send + Sync>;

pub struct Size {
    pub cols: usize,
    pub rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Clone)]
pub struct EventProxy {
    out: Arc<Mutex<Vec<u8>>>,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: AlacEvent) {
        if let AlacEvent::PtyWrite(text) = event
            && let Ok(mut o) = self.out.lock()
        {
            o.extend_from_slice(text.as_bytes());
        }
    }
}

pub struct TermBackend {
    pub term: AlacTerm<EventProxy>,
    parser: Processor,
    proxy_out: Arc<Mutex<Vec<u8>>>,
    rx: Receiver<Vec<u8>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    cols: usize,
    rows: usize,
}

impl TermBackend {
    pub fn spawn_nvim(socket: &Path, cwd: &Path, notifier: Notifier) -> Result<Self, String> {
        let _ = std::fs::remove_file(socket);
        let mut cmd = CommandBuilder::new("nvim");
        cmd.args(["--listen", &socket.to_string_lossy()]);
        Self::spawn_cmd(cmd, cwd, notifier)
    }

    pub fn spawn_shell(cwd: &Path, notifier: Notifier) -> Result<Self, String> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let cmd = CommandBuilder::new(shell);
        Self::spawn_cmd(cmd, cwd, notifier)
    }

    pub fn spawn_program(
        program: &str,
        args: &[&str],
        cwd: &Path,
        notifier: Notifier,
    ) -> Result<Self, String> {
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        Self::spawn_cmd(cmd, cwd, notifier)
    }

    fn spawn_cmd(mut cmd: CommandBuilder, cwd: &Path, notifier: Notifier) -> Result<Self, String> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty failed: {e}"))?;

        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to start process: {e}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to get writer: {e}"))?;

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        notifier();
                    }
                }
            }
            notifier();
        });

        let proxy_out = Arc::new(Mutex::new(Vec::new()));
        let term = AlacTerm::new(
            Config::default(),
            &Size { cols: 80, rows: 24 },
            EventProxy {
                out: proxy_out.clone(),
            },
        );

        Ok(TermBackend {
            term,
            parser: Processor::new(),
            proxy_out,
            rx,
            writer,
            master: pair.master,
            child,
            cols: 80,
            rows: 24,
        })
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.term.resize(Size { cols, rows });
        let _ = self.master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    pub fn pump(&mut self) -> bool {
        let mut advanced = false;
        while let Ok(bytes) = self.rx.try_recv() {
            self.parser.advance(&mut self.term, &bytes);
            advanced = true;
        }
        if let Ok(mut out) = self.proxy_out.lock()
            && !out.is_empty()
        {
            let _ = self.writer.write_all(&out);
            let _ = self.writer.flush();
            out.clear();
        }
        advanced
    }
}

pub fn color_rgb(c: Color) -> Option<(u8, u8, u8)> {
    match c {
        Color::Spec(rgb) => Some((rgb.r, rgb.g, rgb.b)),
        Color::Indexed(i) => Some(indexed(i)),
        Color::Named(n) => {
            let i = match n {
                NamedColor::Black => 0,
                NamedColor::Red => 1,
                NamedColor::Green => 2,
                NamedColor::Yellow => 3,
                NamedColor::Blue => 4,
                NamedColor::Magenta => 5,
                NamedColor::Cyan => 6,
                NamedColor::White => 7,
                NamedColor::BrightBlack => 8,
                NamedColor::BrightRed => 9,
                NamedColor::BrightGreen => 10,
                NamedColor::BrightYellow => 11,
                NamedColor::BrightBlue => 12,
                NamedColor::BrightMagenta => 13,
                NamedColor::BrightCyan => 14,
                NamedColor::BrightWhite => 15,
                _ => return None,
            };
            Some(indexed(i))
        }
    }
}

fn indexed(i: u8) -> (u8, u8, u8) {
    const BASE16: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcd, 0x31, 0x31),
        (0x0d, 0xbc, 0x79),
        (0xe5, 0xe5, 0x10),
        (0x24, 0x72, 0xc8),
        (0xbc, 0x3f, 0xbc),
        (0x11, 0xa8, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x66, 0x66, 0x66),
        (0xf1, 0x4c, 0x4c),
        (0x23, 0xd1, 0x8b),
        (0xf5, 0xf5, 0x43),
        (0x3b, 0x8e, 0xea),
        (0xd6, 0x70, 0xd6),
        (0x29, 0xb8, 0xdb),
        (0xff, 0xff, 0xff),
    ];
    match i {
        0..=15 => BASE16[i as usize],
        16..=231 => {
            let i = i - 16;
            let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (conv(i / 36), conv((i % 36) / 6), conv(i % 6))
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn grid_text(be: &mut TermBackend) -> String {
        use alacritty_terminal::term::cell::Flags;
        let content = be.term.renderable_content();
        let mut rows = vec![String::new(); 24];
        for ind in content.display_iter {
            let r = ind.point.line.0;
            if !(0..24).contains(&r) || ind.cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let c = ind.cell.c;
            rows[r as usize].push(if c == '\0' { ' ' } else { c });
        }
        rows.join("\n")
    }

    #[test]
    fn pty_output_reaches_the_grid() {
        let mut be = TermBackend::spawn_program(
            "sh",
            &["-c", "printf 'hello-term 日本語'; sleep 1"],
            Path::new("/"),
            Arc::new(|| {}),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            be.pump();
            let text = grid_text(&mut be);
            if text.contains("hello-term") && text.contains("日本語") {
                break;
            }
            assert!(Instant::now() < deadline, "grid never showed output: {text}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn feed_reaches_the_child_and_echoes_back() {
        let mut be = TermBackend::spawn_program("cat", &[], Path::new("/"), Arc::new(|| {}));
        let be = be.as_mut().unwrap();
        be.feed(b"roundtrip\r");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            be.pump();
            if grid_text(be).contains("roundtrip") {
                break;
            }
            assert!(Instant::now() < deadline, "echo never arrived");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn color_rgb_maps_named_indexed_and_default() {
        assert_eq!(color_rgb(Color::Named(NamedColor::Red)), Some((0xcd, 0x31, 0x31)));
        assert_eq!(color_rgb(Color::Indexed(15)), Some((0xff, 0xff, 0xff)));
        assert_eq!(color_rgb(Color::Named(NamedColor::Foreground)), None);
        assert_eq!(color_rgb(Color::Named(NamedColor::Background)), None);
    }
}
