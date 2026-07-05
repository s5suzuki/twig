mod diff;
mod graph;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs};

use crate::app::{Pane, Tab, TuiApp};

pub const FOCUS_FG: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
    let cols = Layout::horizontal([
        Constraint::Length(26),
        Constraint::Length(36),
        Constraint::Min(20),
    ])
    .split(frame.area());

    draw_sidebar(frame, app, cols[0]);
    draw_changes(frame, app, cols[1]);
    draw_right(frame, app, cols[2]);
}

fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let block = Block::bordered().title(title);
    if focused {
        block.border_style(Style::default().fg(FOCUS_FG))
    } else {
        block
    }
}

fn draw_sidebar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let block = pane_block("Repositories", app.focus == Pane::Sidebar);
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
    let block = pane_block("Changes", app.focus == Pane::Changes);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let commit_rows: u16 = if app.commit_input.is_some() { 2 } else { 1 };
    let parts = Layout::vertical([
        Constraint::Length(commit_rows),
        Constraint::Min(0),
        Constraint::Length(if app.error.is_some() { 1 } else { 0 }),
    ])
    .split(inner);

    draw_commit_box(frame, app, parts[0]);
    draw_change_list(frame, app, parts[1]);
    if let Some(err) = &app.error {
        frame.render_widget(
            Paragraph::new(Line::styled(err.clone(), Style::default().fg(Color::Red))),
            parts[2],
        );
    }
}

fn draw_commit_box(frame: &mut Frame, app: &TuiApp, area: Rect) {
    match &app.commit_input {
        Some(text) => {
            let lines = vec![
                Line::styled(
                    "Commit message (Enter: commit / Esc: cancel)",
                    Style::default().fg(Color::DarkGray),
                ),
                Line::from(vec![
                    Span::raw(text.clone()),
                    Span::styled("█", Style::default().fg(FOCUS_FG)),
                ]),
            ];
            frame.render_widget(Paragraph::new(lines), area);
        }
        None => {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!("c: commit ({} staged)", app.staged.len()),
                    Style::default().fg(Color::DarkGray),
                )),
                area,
            );
        }
    }
}

fn draw_change_list(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    app.changes_view_rows = area.height as usize;

    struct Row {
        text: String,
        header: bool,
        file_idx: Option<usize>,
    }
    let mut rows: Vec<Row> = Vec::new();
    rows.push(Row {
        text: format!("Staged ({})", app.staged.len()),
        header: true,
        file_idx: None,
    });
    for (i, e) in app.staged.iter().enumerate() {
        rows.push(Row {
            text: format!(" {} {}", e.kind.marker(), e.path),
            header: false,
            file_idx: Some(i),
        });
    }
    rows.push(Row {
        text: format!("Changes ({})", app.unstaged.len()),
        header: true,
        file_idx: None,
    });
    for (i, e) in app.unstaged.iter().enumerate() {
        rows.push(Row {
            text: format!(" {} {}", e.kind.marker(), e.path),
            header: false,
            file_idx: Some(app.staged.len() + i),
        });
    }

    let cursor_row = rows
        .iter()
        .position(|r| r.file_idx == Some(app.changes_cursor))
        .unwrap_or(0);
    let h = area.height as usize;
    if h > 0 {
        if cursor_row < app.changes_scroll {
            app.changes_scroll = cursor_row;
        }
        if cursor_row >= app.changes_scroll + h {
            app.changes_scroll = cursor_row + 1 - h;
        }
        app.changes_scroll = app.changes_scroll.min(rows.len().saturating_sub(1));
    }

    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(app.changes_scroll).take(h) {
        let mut style = if row.header {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        if app.focus == Pane::Changes && i == cursor_row && row.file_idx.is_some() {
            style = style.add_modifier(Modifier::REVERSED);
        }
        lines.push(Line::styled(row.text.clone(), style));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_right(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let block = pane_block("", app.focus == Pane::RightTab);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    let selected = match app.active_tab {
        Tab::Graph => 0,
        Tab::Diff => 1,
    };
    let tabs = Tabs::new(["Graph", "Diff"])
        .select(selected)
        .highlight_style(Style::default().fg(FOCUS_FG).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, rows[0]);

    match app.active_tab {
        Tab::Graph => graph::draw(frame, app, rows[1]),
        Tab::Diff => diff::draw(frame, app, rows[1]),
    }
}
