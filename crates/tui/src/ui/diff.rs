use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use twit_core::Rgb;
use twit_core::highlight::Span as HlSpan;
use twit_core::repo::{DiffRow, LineKind};
use unicode_width::UnicodeWidthChar;

use crate::app::TuiApp;
use crate::ui::FOCUS_FG;

const ADD_BG: Color = Color::Rgb(0x1c, 0x3a, 0x24);
const DEL_BG: Color = Color::Rgb(0x40, 0x20, 0x24);
const ADD_EMPH: Color = Color::Rgb(0x2e, 0x6a, 0x3e);
const DEL_EMPH: Color = Color::Rgb(0x7a, 0x28, 0x35);
const HUNK_FG: Color = Color::Rgb(0x6c, 0x9c, 0xff);
const NO_FG: Color = Color::Rgb(110, 110, 110);
const SEL_BG: Color = Color::Rgb(0x2c, 0x33, 0x4d);
const FIND_BG: Color = Color::Rgb(0x6b, 0x5a, 0x10);

const MAP_TRACK: Color = Color::Rgb(0x22, 0x24, 0x2b);
const MAP_ADD: Color = Color::Rgb(0x3e, 0x8a, 0x52);
const MAP_DEL: Color = Color::Rgb(0x9a, 0x38, 0x45);
const MAP_MOD: Color = Color::Rgb(0x8a, 0x74, 0x2e);
const MAP_MIN_WIDTH: u16 = 28;

pub fn draw(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    if let Some(note) = &app.diff.note {
        frame.render_widget(
            Paragraph::new(Line::styled(
                note.clone(),
                Style::default().fg(Color::DarkGray),
            )),
            area,
        );
        return;
    }
    if app.diff.rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "(no file selected)",
                Style::default().fg(Color::DarkGray),
            )),
            area,
        );
        return;
    }

    let map_w: u16 = if area.width >= MAP_MIN_WIDTH { 1 } else { 0 };
    let text_area = Rect {
        width: area.width - map_w,
        ..area
    };

    let h = area.height as usize;
    app.diff_view_rows = h;
    ensure_cursor_visible(app, h);

    let max_no = app
        .diff
        .rows
        .iter()
        .filter_map(|r| match r {
            DiffRow::Line { old_no, new_no, .. } => {
                Some(old_no.unwrap_or(0).max(new_no.unwrap_or(0)))
            }
            _ => None,
        })
        .max()
        .unwrap_or(1);
    let digits = max_no.to_string().len().max(2);

    let total_w = text_area.width as usize;
    let text_w = total_w
        .saturating_sub(1 + 2 * (digits + 1) + 1)
        .max(20)
        / 2;

    let end = (app.diff_scroll + h).min(app.diff.rows.len());
    app.diff_hl
        .ensure_upto(&app.diff.rows, end.saturating_sub(1));

    let sel = app.diff_nav.highlight(&app.diff.rows);
    let mut lines: Vec<Line> = Vec::new();
    for i in app.diff_scroll..end {
        lines.push(render_row(app, i, digits, text_w, sel));
    }
    frame.render_widget(Paragraph::new(lines), text_area);

    if map_w > 0 {
        let map_area = Rect {
            x: area.x + area.width - map_w,
            width: map_w,
            ..area
        };
        draw_change_map(frame, app, map_area, end);
    }
}

fn draw_change_map(frame: &mut Frame, app: &TuiApp, area: Rect, end: usize) {
    let rows = &app.diff.rows;
    let n = rows.len();
    let h = area.height as usize;
    if n == 0 || h == 0 {
        return;
    }

    let vp_lo = app.diff_scroll * h / n;
    let vp_hi = (end * h / n).max(vp_lo + 1);

    let mut lines: Vec<Line> = Vec::with_capacity(h);
    for y in 0..h {
        let lo = y * n / h;
        let hi = ((y + 1) * n / h).clamp(lo + 1, n);
        let (mut added, mut removed) = (false, false);
        for row in &rows[lo..hi] {
            if let DiffRow::Line { kind, .. } = row {
                match kind {
                    LineKind::Added => added = true,
                    LineKind::Removed => removed = true,
                    LineKind::Changed => {
                        added = true;
                        removed = true;
                    }
                    LineKind::Context => {}
                }
            }
        }
        let bg = match (added, removed) {
            (true, true) => MAP_MOD,
            (true, false) => MAP_ADD,
            (false, true) => MAP_DEL,
            (false, false) => MAP_TRACK,
        };
        let in_vp = y >= vp_lo && y < vp_hi;
        let span = if in_vp {
            Span::styled("▐", Style::default().fg(FOCUS_FG).bg(bg))
        } else {
            Span::styled(" ", Style::default().bg(bg))
        };
        lines.push(Line::from(span));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn ensure_cursor_visible(app: &mut TuiApp, h: usize) {
    if h == 0 {
        return;
    }
    let cursor = app.diff_nav.cursor.min(app.diff.rows.len().saturating_sub(1));
    if app.diff_center {
        app.diff_scroll = cursor.saturating_sub(h / 2);
        app.diff_center = false;
    }
    if cursor < app.diff_scroll {
        app.diff_scroll = cursor;
    }
    if cursor >= app.diff_scroll + h {
        app.diff_scroll = cursor + 1 - h;
    }
    app.diff_scroll = app
        .diff_scroll
        .min(app.diff.rows.len().saturating_sub(1));
}

fn render_row(
    app: &TuiApp,
    i: usize,
    digits: usize,
    text_w: usize,
    sel: Option<(usize, usize)>,
) -> Line<'static> {
    let selected = sel.is_some_and(|(lo, hi)| i >= lo && i <= hi);
    let is_cursor = app.diff_nav.cursor == i;
    let gutter = if is_cursor {
        Span::styled("▌", Style::default().fg(FOCUS_FG))
    } else {
        Span::raw(" ")
    };

    match &app.diff.rows[i] {
        DiffRow::Meta(text) => Line::from(vec![gutter, Span::raw(text.clone())]),
        DiffRow::FileHeader(path) => Line::from(vec![
            gutter,
            Span::styled(
                path.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        DiffRow::Hunk { header, .. } => Line::from(vec![
            gutter,
            Span::styled(header.clone(), Style::default().fg(HUNK_FG)),
        ]),
        DiffRow::Line {
            old_no,
            new_no,
            left,
            right,
            kind,
            left_emph,
            right_emph,
        } => {
            let (left_bg, right_bg) = match kind {
                LineKind::Context => (None, None),
                LineKind::Added => (None, Some(ADD_BG)),
                LineKind::Removed => (Some(DEL_BG), None),
                LineKind::Changed => (Some(DEL_BG), Some(ADD_BG)),
            };
            let overlay = if selected { Some(SEL_BG) } else { None };

            let find = |text: Option<&str>| match (&app.diff_find, text) {
                (Some(q), Some(t)) => crate::app::find_ranges(q, t),
                _ => Vec::new(),
            };
            let mut spans = vec![gutter];
            spans.push(lineno_span(*old_no, digits));
            spans.extend(cell_spans(
                left.as_deref(),
                app.diff_hl.left(i),
                left_emph,
                &find(left.as_deref()),
                left_bg,
                DEL_EMPH,
                overlay,
                text_w,
            ));
            spans.push(lineno_span(*new_no, digits));
            spans.extend(cell_spans(
                right.as_deref(),
                app.diff_hl.right(i),
                right_emph,
                &find(right.as_deref()),
                right_bg,
                ADD_EMPH,
                overlay,
                text_w,
            ));
            Line::from(spans)
        }
    }
}

fn lineno_span(no: Option<u32>, digits: usize) -> Span<'static> {
    let text = match no {
        Some(n) => format!("{n:>digits$} "),
        None => " ".repeat(digits + 1),
    };
    Span::styled(text, Style::default().fg(NO_FG))
}

#[allow(clippy::too_many_arguments)]
fn cell_spans(
    text: Option<&str>,
    syn: &[HlSpan],
    emph: &[Range<usize>],
    find: &[Range<usize>],
    base_bg: Option<Color>,
    emph_bg: Color,
    overlay: Option<Color>,
    width: usize,
) -> Vec<Span<'static>> {
    let text = text.unwrap_or("");
    let mut out = Vec::new();
    let mut run = String::new();
    let mut run_style = Style::default();
    let mut used = 0usize;

    let flush = |out: &mut Vec<Span<'static>>, run: &mut String, style: Style| {
        if !run.is_empty() {
            out.push(Span::styled(std::mem::take(run), style));
        }
    };

    for (b, ch) in text.char_indices() {
        let cw = ch.width().unwrap_or(0);
        if used + cw > width {
            break;
        }
        let fg = syn
            .iter()
            .find(|&&(s, e, _)| b >= s && b < e)
            .map(|&(_, _, c)| rgb(c));
        let bg = if find.iter().any(|r| b >= r.start && b < r.end) {
            Some(FIND_BG)
        } else if emph.iter().any(|r| b >= r.start && b < r.end) {
            Some(emph_bg)
        } else {
            overlay.or(base_bg)
        };
        let mut style = Style::default();
        if let Some(fg) = fg {
            style = style.fg(fg);
        }
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        if style != run_style {
            flush(&mut out, &mut run, run_style);
            run_style = style;
        }
        run.push(ch);
        used += cw;
    }
    flush(&mut out, &mut run, run_style);

    if used < width {
        let mut pad_style = Style::default();
        if let Some(bg) = overlay.or(base_bg) {
            pad_style = pad_style.bg(bg);
        }
        out.push(Span::styled(" ".repeat(width - used), pad_style));
    }
    out
}

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}
