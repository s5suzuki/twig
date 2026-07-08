mod diff;
mod graph;
mod help;
mod search;
mod settings;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Padding, Paragraph, Tabs, Wrap};

use crate::app::{ChangesItem, Pane, Tab, TuiApp, View, ViewMode};

pub const FOCUS_FG: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
    app.editor_cursor_shape = None;
    let mut area = frame.area();
    if app.seq.is_some() {
        let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        draw_seq_banner(frame, app, parts[0]);
        area = parts[1];
    }
    if app.remote.is_some() {
        let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        draw_remote_bar(frame, app, parts[1]);
        area = parts[0];
    }
    let popup_prompt = app.prompt.as_ref().is_some_and(|(k, _)| k.wants_popup());
    if app.prompt.is_some() && !popup_prompt {
        let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        draw_prompt_bar(frame, app, parts[1]);
        area = parts[0];
    }
    if app.help_open {
        help::draw(frame, app, area);
        return;
    }
    if app.settings_open {
        settings::draw(frame, app, area);
        return;
    }
    match app.view_mode {
        ViewMode::All => draw_all(frame, app, area),
        ViewMode::Single(view) => draw_single(frame, app, view, area),
    }
    if popup_prompt {
        draw_prompt_popup(frame, app, area);
    }
}

fn draw_prompt_popup(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let Some((kind, input)) = &app.prompt else {
        return;
    };
    let surface = Color::Rgb(49, 50, 68);
    let shadow = Color::Rgb(17, 17, 27);
    let text = Color::Rgb(205, 214, 244);

    let w = area.width.saturating_sub(6).clamp(24, 72).max(1);
    let inner_w = w.saturating_sub(4).max(1) as usize;

    let mut lines: Vec<Line> = wrap_plain(&kind.label(), inner_w)
        .into_iter()
        .map(|c| Line::styled(c, Style::default().fg(text)))
        .collect();
    if kind.wants_text() {
        lines.push(Line::raw(""));
        let field = format!("{input}█");
        for chunk in wrap_plain(&field, inner_w) {
            lines.push(Line::styled(chunk, Style::default().fg(FOCUS_FG)));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(kind.hint(), Style::default().fg(Color::Gray)));

    let h = (lines.len() as u16 + 2).clamp(3, area.height.max(3));
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let rect = Rect::new(x, y, w, h);

    let shadow_rect = area.intersection(Rect::new(x + 2, y + 1, w, h));
    frame.render_widget(Clear, shadow_rect);
    frame.render_widget(
        Block::default().style(Style::default().bg(shadow)),
        shadow_rect,
    );

    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .padding(Padding::horizontal(1))
        .border_style(Style::default().fg(FOCUS_FG).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(surface));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(surface))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for word in text.split_whitespace() {
        let wlen = word.chars().count();
        if wlen > width {
            if cur_len > 0 {
                out.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            for ch in word.chars() {
                if cur_len == width {
                    out.push(std::mem::take(&mut cur));
                    cur_len = 0;
                }
                cur.push(ch);
                cur_len += 1;
            }
        } else if cur_len == 0 {
            cur.push_str(word);
            cur_len = wlen;
        } else if cur_len + 1 + wlen <= width {
            cur.push(' ');
            cur.push_str(word);
            cur_len += 1 + wlen;
        } else {
            out.push(std::mem::take(&mut cur));
            cur.push_str(word);
            cur_len = wlen;
        }
    }
    if cur_len > 0 || out.is_empty() {
        out.push(cur);
    }
    out
}

fn draw_seq_banner(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let Some((kind, conflicts)) = &app.seq else {
        return;
    };
    let mut text = format!(
        "{} in progress — C: continue / A: abort",
        crate::app::seq_label(*kind)
    );
    if !conflicts.is_empty() {
        text.push_str(&format!(
            "  |  conflicts ({}): {}",
            conflicts.len(),
            conflicts.join(", ")
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::styled(
            text,
            Style::default().fg(Color::Black).bg(Color::Yellow),
        )),
        area,
    );
}

fn draw_remote_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let Some(job) = &app.remote else {
        return;
    };
    let text = match job.progress {
        Some((r, t)) if t > 0 => format!("{} {r}/{t}…", job.kind.running()),
        _ => format!("{}…", job.kind.running()),
    };
    frame.render_widget(
        Paragraph::new(Line::styled(text, Style::default().fg(Color::Cyan))),
        area,
    );
}

fn draw_prompt_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let Some((kind, input)) = &app.prompt else {
        return;
    };
    let mut spans = vec![Span::styled(
        format!("{} ", kind.label()),
        Style::default().fg(Color::Yellow),
    )];
    if kind.wants_text() {
        spans.push(Span::raw(input.clone()));
        spans.push(Span::styled("█", Style::default().fg(FOCUS_FG)));
        spans.push(Span::styled(
            "  (Enter: confirm / Esc: cancel)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_all(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let cols = Layout::horizontal([
        Constraint::Length(26),
        Constraint::Length(36),
        Constraint::Min(20),
    ])
    .split(area);

    draw_sidebar(frame, app, cols[0]);
    draw_changes(frame, app, cols[1]);
    draw_right(frame, app, cols[2]);
}

fn draw_single(frame: &mut Frame, app: &mut TuiApp, view: View, mut area: Rect) {
    if view != View::Changes
        && let Some(err) = &app.error
    {
        let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        frame.render_widget(
            Paragraph::new(Line::styled(err.clone(), Style::default().fg(Color::Red))),
            parts[1],
        );
        area = parts[0];
    }
    match view {
        View::Sidebar => draw_sidebar(frame, app, area),
        View::Changes => draw_changes(frame, app, area),
        View::Main => draw_right(frame, app, area),
        View::Graph => graph::draw(frame, app, area),
        View::Diff => diff::draw(frame, app, area),
    }
}

fn pane_block<'a>(app: &TuiApp, title: &'a str, focused: bool) -> Block<'a> {
    if matches!(app.view_mode, ViewMode::Single(_)) {
        return Block::new();
    }
    let block = Block::bordered().title(title);
    if focused {
        block.border_style(Style::default().fg(FOCUS_FG))
    } else {
        block
    }
}

fn draw_sidebar(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let block = pane_block(app, "Repositories", app.focus == Pane::Sidebar);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.sidebar_view_rows = inner.height as usize;

    let rows = app.sidebar_rows();
    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().take(inner.height as usize) {
        let selected = row.path == app.selected;
        let cursor = app.focus == Pane::Sidebar && i == app.sidebar_cursor.min(rows.len() - 1);
        let mut style = Style::default();
        if selected {
            style = style.add_modifier(Modifier::BOLD).fg(FOCUS_FG);
        }
        if cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        lines.push(Line::styled(row.label.clone(), style));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_changes(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let block = pane_block(app, "Changes", app.focus == Pane::Changes);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(if app.error.is_some() { 1 } else { 0 }),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Line::styled(
            format!(
                "c: commit / a: amend / z: stash ({} staged)",
                app.staged.len()
            ),
            Style::default().fg(Color::DarkGray),
        )),
        parts[0],
    );
    draw_change_list(frame, app, parts[1]);
    if let Some(err) = &app.error {
        frame.render_widget(
            Paragraph::new(Line::styled(err.clone(), Style::default().fg(Color::Red))),
            parts[2],
        );
    }
}

fn draw_change_list(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    app.changes_view_rows = area.height as usize;

    let items = app.changes_items();
    let cursor = app.changes_cursor.min(items.len().saturating_sub(1));
    let h = area.height as usize;
    if h > 0 {
        if cursor < app.changes_scroll {
            app.changes_scroll = cursor;
        }
        if cursor >= app.changes_scroll + h {
            app.changes_scroll = cursor + 1 - h;
        }
        app.changes_scroll = app.changes_scroll.min(items.len().saturating_sub(1));
    }

    let marker = |path: &str, staged: bool| {
        let entries = if staged { &app.staged } else { &app.unstaged };
        entries
            .iter()
            .find(|e| e.path == path)
            .map(|e| e.kind.marker())
            .unwrap_or(' ')
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, item) in items.iter().enumerate().skip(app.changes_scroll).take(h) {
        let (text, header) = match item {
            ChangesItem::Group { staged: true } => (format!("Staged ({})", app.staged.len()), true),
            ChangesItem::Group { staged: false } => {
                (format!("Changes ({})", app.unstaged.len()), true)
            }
            ChangesItem::StashHeader => (format!("Stashes ({})", app.stashes.len()), true),
            ChangesItem::Folder {
                name, open, depth, ..
            } => (
                format!(
                    "{}{} {}/",
                    "  ".repeat(*depth),
                    if *open { "▾" } else { "▸" },
                    name
                ),
                false,
            ),
            ChangesItem::File {
                path,
                staged,
                depth,
            } => {
                let name = path.rsplit('/').next().unwrap_or(path);
                (
                    format!("{}{} {}", "  ".repeat(*depth), marker(path, *staged), name),
                    false,
                )
            }
            ChangesItem::Stash(index) => {
                let msg = app
                    .stashes
                    .iter()
                    .find(|s| s.index == *index)
                    .map(|s| s.message.as_str())
                    .unwrap_or("");
                (format!("  stash@{{{index}}} {msg}"), false)
            }
        };
        let mut style = if header {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        if app.focus == Pane::Changes && i == cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        lines.push(Line::styled(text, style));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_right(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let block = pane_block(app, "", app.focus == Pane::RightTab);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    let selected = match app.active_tab {
        Tab::Graph => 0,
        Tab::Diff => 1,
        Tab::Search => 2,
        Tab::Editor => 3,
    };
    let tabs = Tabs::new(["Graph", "Diff", "Search", "Editor"])
        .select(selected)
        .highlight_style(Style::default().fg(FOCUS_FG).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, rows[0]);

    match app.active_tab {
        Tab::Graph => graph::draw(frame, app, rows[1]),
        Tab::Diff => diff::draw(frame, app, rows[1]),
        Tab::Search => search::draw(frame, app, rows[1]),
        Tab::Editor => draw_editor(frame, app, rows[1]),
    }
}

fn draw_editor(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    app.editor_area = Some(area);
    let alive = app.term.as_mut().is_some_and(|t| t.is_alive());
    if !alive {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "(nvim is not running — re-enter this tab to restart it)",
                Style::default().fg(Color::DarkGray),
            )),
            area,
        );
        return;
    }
    let focused = app.focus == Pane::RightTab;
    let term = app.term.as_mut().unwrap();
    term.pump();
    let cursor = term.draw(frame.buffer_mut(), area, focused);
    if let Some((x, y, style)) = cursor {
        frame.set_cursor_position((x, y));
        app.editor_cursor_shape = Some(style);
    }
}
