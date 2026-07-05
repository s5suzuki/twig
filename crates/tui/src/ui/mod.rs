mod diff;
mod graph;
mod help;
mod search;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs};

use crate::app::{Pane, Tab, TuiApp, View, ViewMode};

pub const FOCUS_FG: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
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
    if app.prompt.is_some() {
        let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        draw_prompt_bar(frame, app, parts[1]);
        area = parts[0];
    }
    if app.help_open {
        help::draw(frame, app, area);
        return;
    }
    match app.view_mode {
        ViewMode::All => draw_all(frame, app, area),
        ViewMode::Single(view) => draw_single(frame, app, view, area),
    }
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
    if !app.stashes.is_empty() {
        rows.push(Row {
            text: format!("Stashes ({})", app.stashes.len()),
            header: true,
            file_idx: None,
        });
        let base = app.staged.len() + app.unstaged.len();
        for (i, s) in app.stashes.iter().enumerate() {
            rows.push(Row {
                text: format!(" stash@{{{}}} {}", s.index, s.message),
                header: false,
                file_idx: Some(base + i),
            });
        }
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
    let block = pane_block(app, "", app.focus == Pane::RightTab);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    let selected = match app.active_tab {
        Tab::Graph => 0,
        Tab::Diff => 1,
        Tab::Search => 2,
    };
    let tabs = Tabs::new(["Graph", "Diff", "Search"])
        .select(selected)
        .highlight_style(Style::default().fg(FOCUS_FG).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, rows[0]);

    match app.active_tab {
        Tab::Graph => graph::draw(frame, app, rows[1]),
        Tab::Diff => diff::draw(frame, app, rows[1]),
        Tab::Search => search::draw(frame, app, rows[1]),
    }
}
