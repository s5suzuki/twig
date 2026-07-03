use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use regex::Regex;

use alacritty_terminal::Term as AlacTerm;
use alacritty_terminal::event::{Event as AlacEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor};
use egui::{Align2, Color32, FontId, Pos2, Rect, pos2, vec2};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

const FONT_SIZE: f32 = 13.0;

struct UrlHit {
    url: String,
    row: i32,
    start: usize,
    end: usize,
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:https?|ftp|file)://[^\s]+").unwrap())
}

struct Size {
    cols: usize,
    rows: usize,
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
struct EventProxy {
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

pub struct Term {
    term: AlacTerm<EventProxy>,
    parser: Processor,
    proxy_out: Arc<Mutex<Vec<u8>>>,
    rx: Receiver<Vec<u8>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    cols: usize,
    rows: usize,
    mouse_pressed: Option<u8>,
    last_mouse_cell: Option<(usize, usize)>,
    selecting: bool,
    preedit: String,
}

impl Term {
    pub fn spawn(
        socket: &Path,
        cwd: &Path,
        ctx: &egui::Context,
        repaint_gate: Arc<AtomicBool>,
    ) -> Result<Term, String> {
        let _ = std::fs::remove_file(socket);

        let mut cmd = CommandBuilder::new("nvim");
        cmd.args(["--listen", &socket.to_string_lossy()]);
        Self::spawn_cmd(cmd, cwd, ctx, repaint_gate)
    }

    pub fn spawn_shell(
        cwd: &Path,
        ctx: &egui::Context,
        repaint_gate: Arc<AtomicBool>,
    ) -> Result<Term, String> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let cmd = CommandBuilder::new(shell);
        Self::spawn_cmd(cmd, cwd, ctx, repaint_gate)
    }

    fn spawn_cmd(
        mut cmd: CommandBuilder,
        cwd: &Path,
        ctx: &egui::Context,
        repaint_gate: Arc<AtomicBool>,
    ) -> Result<Term, String> {
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
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        if repaint_gate.load(Ordering::Relaxed) {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            if repaint_gate.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
        });

        let proxy_out = Arc::new(Mutex::new(Vec::new()));
        let term = AlacTerm::new(
            Config::default(),
            &Size { cols: 80, rows: 24 },
            EventProxy {
                out: proxy_out.clone(),
            },
        );

        Ok(Term {
            term,
            parser: Processor::new(),
            proxy_out,
            rx,
            writer,
            master: pair.master,
            child,
            cols: 80,
            rows: 24,
            mouse_pressed: None,
            last_mouse_cell: None,
            selecting: false,
            preedit: String::new(),
        })
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    fn resize(&mut self, cols: usize, rows: usize) {
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

    pub fn ui(&mut self, ui: &mut egui::Ui, active: bool) -> bool {
        while let Ok(bytes) = self.rx.try_recv() {
            self.parser.advance(&mut self.term, &bytes);
        }
        if let Ok(mut out) = self.proxy_out.lock()
            && !out.is_empty()
        {
            let _ = self.writer.write_all(&out);
            let _ = self.writer.flush();
            out.clear();
        }

        let font = FontId::monospace(FONT_SIZE);
        let (cw, rh) = ui
            .ctx()
            .fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let avail = ui.available_size();
        let cols = (((avail.x - 1.0) / cw).floor() as usize).max(1);
        let rows = (((avail.y - 1.0) / rh).floor() as usize).max(1);
        if cols != self.cols || rows != self.rows {
            self.resize(cols, rows);
        }

        let id = ui.make_persistent_id("embedded_term");
        let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
        let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
        let clicked = resp.clicked();
        if clicked || active {
            if !resp.has_focus() {
                resp.request_focus();
            }
        } else if resp.has_focus() {
            resp.surrender_focus();
        }
        let focused = active && resp.has_focus();

        let ctrl = ui.input(|i| i.modifiers.ctrl);
        let hover_pos = if resp.hovered() {
            ui.input(|i| i.pointer.latest_pos())
        } else {
            None
        };
        let url_hit = if ctrl {
            hover_pos.and_then(|p| self.url_at(p, rect, cw, rh))
        } else {
            None
        };
        let mut open_url: Option<String> = None;
        if let Some(hit) = &url_hit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                open_url = Some(hit.url.clone());
            }
        }

        self.handle_mouse(ui, &resp, rect, cw, rh, url_hit.is_some());

        let default_bg = ui.visuals().panel_fill;
        let default_fg = ui.visuals().strong_text_color();

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, default_bg);

        let mut glyphs: Vec<(f32, f32, char, Color32)> = Vec::new();
        let content = self.term.renderable_content();
        let display_offset = content.display_offset as i32;
        let selection = content.selection;
        let sel_bg = ui.visuals().selection.bg_fill;
        for ind in content.display_iter {
            let p = ind.point;
            let screen_row = p.line.0 + display_offset;
            if screen_row < 0 || screen_row >= self.rows as i32 {
                continue;
            }
            let x = rect.left() + p.column.0 as f32 * cw;
            let y = rect.top() + screen_row as f32 * rh;
            let cell = ind.cell;

            let selected = selection.is_some_and(|r| r.contains(p));
            let bg = if selected {
                sel_bg
            } else {
                to_color(cell.bg, default_bg)
            };
            if bg != default_bg {
                painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(cw, rh)), 0.0, bg);
            }
            if cell.c != ' ' && cell.c != '\0' {
                glyphs.push((x, y, cell.c, to_color(cell.fg, default_fg)));
            }
        }
        for (x, y, c, fg) in glyphs {
            painter.text(pos2(x, y), Align2::LEFT_TOP, c, font.clone(), fg);
        }
        if let Some(hit) = &url_hit {
            let y = rect.top() + hit.row as f32 * rh + rh - 1.0;
            let x0 = rect.left() + hit.start as f32 * cw;
            let x1 = rect.left() + (hit.end + 1) as f32 * cw;
            painter.line_segment(
                [pos2(x0, y), pos2(x1, y)],
                egui::Stroke::new(1.0, Color32::from_rgb(0x56, 0x9c, 0xd6)),
            );
        }
        let cur = content.cursor;
        let cursor_row = cur.point.line.0 + display_offset;
        let cursor_rect = {
            let x = rect.left() + cur.point.column.0 as f32 * cw;
            let y = rect.top() + cursor_row.clamp(0, self.rows as i32 - 1) as f32 * rh;
            Rect::from_min_size(pos2(x, y), vec2(cw, rh))
        };
        if cur.shape != CursorShape::Hidden && cursor_row >= 0 && cursor_row < self.rows as i32 {
            let x = rect.left() + cur.point.column.0 as f32 * cw;
            let y = rect.top() + cursor_row as f32 * rh;
            let solid = if focused {
                default_fg
            } else {
                default_fg.gamma_multiply(0.5)
            };
            match cur.shape {
                CursorShape::Beam => {
                    painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(2.0, rh)), 0.0, solid);
                }
                CursorShape::Underline => {
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, y + rh - 2.0), vec2(cw, 2.0)),
                        0.0,
                        solid,
                    );
                }
                _ => {
                    let cell = Rect::from_min_size(pos2(x, y), vec2(cw, rh));
                    if focused && cur.shape == CursorShape::Block {
                        painter.rect_filled(
                            cell,
                            0.0,
                            Color32::from_rgba_unmultiplied(
                                default_fg.r(),
                                default_fg.g(),
                                default_fg.b(),
                                130,
                            ),
                        );
                    } else {
                        for r in [
                            Rect::from_min_size(pos2(x, y), vec2(cw, 1.0)),
                            Rect::from_min_size(pos2(x, y + rh - 1.0), vec2(cw, 1.0)),
                            Rect::from_min_size(pos2(x, y), vec2(1.0, rh)),
                            Rect::from_min_size(pos2(x + cw - 1.0, y), vec2(1.0, rh)),
                        ] {
                            painter.rect_filled(r, 0.0, solid);
                        }
                    }
                }
            }
        }

        if focused && !self.preedit.is_empty() {
            let galley = painter.layout_no_wrap(self.preedit.clone(), font.clone(), default_fg);
            let top_left = cursor_rect.left_top();
            let bg = Rect::from_min_size(top_left, galley.size());
            painter.rect_filled(bg, 0.0, default_bg);
            painter.galley(top_left, galley, default_fg);
            painter.line_segment(
                [
                    pos2(bg.left(), bg.bottom() - 1.0),
                    pos2(bg.right(), bg.bottom() - 1.0),
                ],
                egui::Stroke::new(1.0, default_fg),
            );
        }

        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            ui.ctx().output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    rect: cursor_rect,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });
            ui.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Ime(ime) = ev {
                        match ime {
                            egui::ImeEvent::Preedit { text, .. } => self.preedit = text.clone(),
                            egui::ImeEvent::Commit(_) => self.preedit.clear(),
                            _ => {}
                        }
                    }
                }
            });
            let sel_text = self.term.selection_to_string();
            let has_sel = sel_text.as_deref().is_some_and(|t| !t.is_empty());
            let mut bytes: Vec<u8> = Vec::new();
            let mut do_copy = false;
            ui.input(|i| {
                for ev in &i.events {
                    match ev {
                        egui::Event::Copy => {
                            if has_sel {
                                do_copy = true;
                            } else {
                                bytes.push(0x03);
                            }
                        }
                        egui::Event::Cut => bytes.push(0x18),
                        _ => {}
                    }
                }
            });
            if do_copy {
                if let Some(t) = sel_text {
                    ui.ctx().copy_text(t);
                }
                self.term.selection = None;
            }
            bytes.extend(input_to_bytes(ui));
            if !bytes.is_empty() {
                self.term.selection = None;
                if self.term.grid().display_offset() != 0 {
                    self.term.scroll_display(Scroll::Bottom);
                }
                let _ = self.writer.write_all(&bytes);
                let _ = self.writer.flush();
            }
        } else if !self.preedit.is_empty() {
            self.preedit.clear();
        }

        if let Some(url) = open_url {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }

        clicked
    }

    fn url_at(&self, pos: Pos2, rect: Rect, cw: f32, rh: f32) -> Option<UrlHit> {
        if !rect.contains(pos) {
            return None;
        }
        let col = ((pos.x - rect.left()) / cw).floor() as i32;
        let row = ((pos.y - rect.top()) / rh).floor() as i32;
        if col < 0 || row < 0 || col >= self.cols as i32 || row >= self.rows as i32 {
            return None;
        }
        let mut line = vec![' '; self.cols];
        let content = self.term.renderable_content();
        let display_offset = content.display_offset as i32;
        for ind in content.display_iter {
            if ind.point.line.0 + display_offset != row {
                continue;
            }
            if let Some(slot) = line.get_mut(ind.point.column.0) {
                let c = ind.cell.c;
                *slot = if c == '\0' { ' ' } else { c };
            }
        }
        let text: String = line.into_iter().collect();
        for m in url_regex().find_iter(&text) {
            let url = m
                .as_str()
                .trim_end_matches(|c: char| ".,;:!?)]}>'\"".contains(c));
            if url.is_empty() {
                continue;
            }
            let start = text[..m.start()].chars().count();
            let len = url.chars().count();
            if (col as usize) >= start && (col as usize) < start + len {
                return Some(UrlHit {
                    url: url.to_string(),
                    row,
                    start,
                    end: start + len - 1,
                });
            }
        }
        None
    }

    fn handle_mouse(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: Rect,
        cw: f32,
        rh: f32,
        link_mode: bool,
    ) {
        let mode = *self.term.mode();
        let report = mode.intersects(
            TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
        );
        let display_offset = self.term.grid().display_offset() as i32;

        let mut events = Vec::new();
        ui.input(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::PointerButton { .. }
                    | egui::Event::PointerMoved(_)
                    | egui::Event::MouseWheel { .. } => events.push(ev.clone()),
                    _ => {}
                }
            }
        });
        let pointer_pos = ui.input(|i| i.pointer.latest_pos());

        let mut bytes: Vec<u8> = Vec::new();
        for ev in events {
            match ev {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    if link_mode && button == egui::PointerButton::Primary {
                        continue;
                    }
                    let primary = button == egui::PointerButton::Primary;
                    if primary && (!report || modifiers.shift) {
                        if rect.contains(pos) && pressed {
                            let (point, side) = pos_to_point(
                                pos,
                                rect,
                                cw,
                                rh,
                                self.cols,
                                self.rows,
                                display_offset,
                            );
                            self.term.selection =
                                Some(Selection::new(SelectionType::Simple, point, side));
                            self.selecting = true;
                        } else if !pressed && self.selecting {
                            self.selecting = false;
                            self.copy_selection(ui);
                        }
                        continue;
                    }
                    if !report || !rect.contains(pos) {
                        continue;
                    }
                    if pressed {
                        self.term.selection = None;
                    }
                    let base = match button {
                        egui::PointerButton::Primary => 0u8,
                        egui::PointerButton::Middle => 1,
                        egui::PointerButton::Secondary => 2,
                        _ => continue,
                    };
                    let (col, row) = pos_to_cell(pos, rect, cw, rh, self.cols, self.rows);
                    let code = base + mouse_mods(&modifiers);
                    if pressed {
                        self.mouse_pressed = Some(base);
                        self.last_mouse_cell = Some((col, row));
                    } else {
                        self.mouse_pressed = None;
                    }
                    bytes.extend_from_slice(&sgr_mouse(code, col, row, pressed));
                }
                egui::Event::PointerMoved(pos) => {
                    if self.selecting {
                        let (point, side) =
                            pos_to_point(pos, rect, cw, rh, self.cols, self.rows, display_offset);
                        if let Some(sel) = self.term.selection.as_mut() {
                            sel.update(point, side);
                        }
                        continue;
                    }
                    if !report || !rect.contains(pos) {
                        continue;
                    }
                    let motion = mode.contains(TermMode::MOUSE_MOTION)
                        || (mode.contains(TermMode::MOUSE_DRAG) && self.mouse_pressed.is_some());
                    if !motion {
                        continue;
                    }
                    let cell = pos_to_cell(pos, rect, cw, rh, self.cols, self.rows);
                    if self.last_mouse_cell == Some(cell) {
                        continue;
                    }
                    self.last_mouse_cell = Some(cell);
                    let base = self.mouse_pressed.unwrap_or(3);
                    bytes.extend_from_slice(&sgr_mouse(base + 32, cell.0, cell.1, true));
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    if !resp.hovered() {
                        continue;
                    }
                    let lines = match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / rh,
                        egui::MouseWheelUnit::Page => delta.y * self.rows as f32,
                    };
                    let steps = lines.round() as i32;
                    if steps == 0 {
                        continue;
                    }
                    if report {
                        let btn = if steps > 0 { 64 } else { 65 } + mouse_mods(&modifiers);
                        let (col, row) = pointer_pos
                            .map(|p| pos_to_cell(p, rect, cw, rh, self.cols, self.rows))
                            .unwrap_or((1, 1));
                        for _ in 0..steps.abs() {
                            bytes.extend_from_slice(&sgr_mouse(btn, col, row, true));
                        }
                    } else {
                        self.term.scroll_display(Scroll::Delta(steps));
                    }
                }
                _ => {}
            }
        }

        if !bytes.is_empty() {
            let _ = self.writer.write_all(&bytes);
            let _ = self.writer.flush();
        }
    }

    fn copy_selection(&self, ui: &egui::Ui) {
        if let Some(text) = self.term.selection_to_string()
            && !text.is_empty()
        {
            ui.ctx().copy_text(text);
        }
    }
}

fn pos_to_point(
    pos: Pos2,
    rect: Rect,
    cw: f32,
    rh: f32,
    cols: usize,
    rows: usize,
    display_offset: i32,
) -> (Point, Side) {
    let rel_x = (pos.x - rect.left()).max(0.0);
    let col = ((rel_x / cw).floor()).clamp(0.0, (cols - 1) as f32) as usize;
    let screen_row = (((pos.y - rect.top()) / rh).floor()).clamp(0.0, (rows - 1) as f32) as i32;
    let side = if rel_x - col as f32 * cw > cw / 2.0 {
        Side::Right
    } else {
        Side::Left
    };
    (
        Point::new(Line(screen_row - display_offset), Column(col)),
        side,
    )
}

fn pos_to_cell(
    pos: Pos2,
    rect: Rect,
    cw: f32,
    rh: f32,
    cols: usize,
    rows: usize,
) -> (usize, usize) {
    let col = (((pos.x - rect.left()) / cw).floor()).clamp(0.0, (cols - 1) as f32) as usize + 1;
    let row = (((pos.y - rect.top()) / rh).floor()).clamp(0.0, (rows - 1) as f32) as usize + 1;
    (col, row)
}

fn mouse_mods(m: &egui::Modifiers) -> u8 {
    let mut b = 0;
    if m.shift {
        b += 4;
    }
    if m.alt {
        b += 8;
    }
    if m.ctrl {
        b += 16;
    }
    b
}

fn sgr_mouse(button: u8, col: usize, row: usize, pressed: bool) -> Vec<u8> {
    let f = if pressed { 'M' } else { 'm' };
    format!("\x1b[<{button};{col};{row}{f}").into_bytes()
}

fn input_to_bytes(ui: &egui::Ui) -> Vec<u8> {
    let mut out = Vec::new();
    ui.input(|i| {
        for ev in &i.events {
            match ev {
                egui::Event::Text(t) => out.extend_from_slice(t.as_bytes()),
                egui::Event::Ime(egui::ImeEvent::Commit(s)) => out.extend_from_slice(s.as_bytes()),
                egui::Event::Paste(s) => out.extend_from_slice(s.as_bytes()),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(b) = key_to_bytes(*key, modifiers) {
                        out.extend_from_slice(&b);
                    }
                }
                _ => {}
            }
        }
    });
    out
}

fn key_to_bytes(key: egui::Key, mods: &egui::Modifiers) -> Option<Vec<u8>> {
    use egui::Key::*;
    let b: &[u8] = match key {
        Enter => b"\r",
        Backspace => &[0x7f],
        Escape => &[0x1b],
        Tab => {
            if mods.shift {
                b"\x1b[Z"
            } else {
                b"\t"
            }
        }
        ArrowUp => b"\x1b[A",
        ArrowDown => b"\x1b[B",
        ArrowRight => b"\x1b[C",
        ArrowLeft => b"\x1b[D",
        Home => b"\x1b[H",
        End => b"\x1b[F",
        PageUp => b"\x1b[5~",
        PageDown => b"\x1b[6~",
        Delete => b"\x1b[3~",
        Insert => b"\x1b[2~",
        _ => {
            if mods.ctrl
                && let Some(c) = ctrl_byte(key)
            {
                return Some(vec![c]);
            }
            return None;
        }
    };
    Some(b.to_vec())
}

fn ctrl_byte(key: egui::Key) -> Option<u8> {
    use egui::Key::*;
    let idx: u8 = match key {
        A => 1,
        B => 2,
        C => 3,
        D => 4,
        E => 5,
        F => 6,
        G => 7,
        H => 8,
        I => 9,
        J => 10,
        K => 11,
        L => 12,
        M => 13,
        N => 14,
        O => 15,
        P => 16,
        Q => 17,
        R => 18,
        S => 19,
        T => 20,
        U => 21,
        V => 22,
        W => 23,
        X => 24,
        Y => 25,
        Z => 26,
        _ => return None,
    };
    Some(idx)
}

fn to_color(c: AnsiColor, default: Color32) -> Color32 {
    match c {
        AnsiColor::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => indexed(i),
        AnsiColor::Named(n) => match n {
            NamedColor::Foreground => default,
            NamedColor::Background => default,
            NamedColor::Black => indexed(0),
            NamedColor::Red => indexed(1),
            NamedColor::Green => indexed(2),
            NamedColor::Yellow => indexed(3),
            NamedColor::Blue => indexed(4),
            NamedColor::Magenta => indexed(5),
            NamedColor::Cyan => indexed(6),
            NamedColor::White => indexed(7),
            NamedColor::BrightBlack => indexed(8),
            NamedColor::BrightRed => indexed(9),
            NamedColor::BrightGreen => indexed(10),
            NamedColor::BrightYellow => indexed(11),
            NamedColor::BrightBlue => indexed(12),
            NamedColor::BrightMagenta => indexed(13),
            NamedColor::BrightCyan => indexed(14),
            NamedColor::BrightWhite => indexed(15),
            _ => default,
        },
    }
}

fn indexed(i: u8) -> Color32 {
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
        0..=15 => {
            let (r, g, b) = BASE16[i as usize];
            Color32::from_rgb(r, g, b)
        }
        16..=231 => {
            let i = i - 16;
            let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(conv(i / 36), conv((i % 36) / 6), conv(i % 6))
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Color32::from_gray(v)
        }
    }
}
