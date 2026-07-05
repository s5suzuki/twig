use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use twig_core::repo::{GraphRow, RefKind, Segment};

use crate::app::{GraphItem, Pane, TuiApp};

const LANE_COLORS: [Color; 8] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::Red,
    Color::LightCyan,
    Color::LightMagenta,
];

fn lane_color(idx: usize) -> Color {
    LANE_COLORS[idx % LANE_COLORS.len()]
}

pub fn draw(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let h = area.height as usize;
    let visible = h.div_ceil(2).max(1);
    app.graph_view_rows = visible;
    let items = app.graph_items();
    if items.is_empty() || h == 0 {
        return;
    }

    let cursor = app.graph_cursor.min(items.len() - 1);
    if cursor < app.graph_scroll {
        app.graph_scroll = cursor;
    }
    if cursor >= app.graph_scroll + visible {
        app.graph_scroll = cursor + 1 - visible;
    }
    app.graph_scroll = app.graph_scroll.min(items.len() - 1);

    let mut lines: Vec<Line> = Vec::new();
    for (i, item) in items.iter().enumerate().skip(app.graph_scroll) {
        if lines.len() >= h {
            break;
        }
        let focused = app.focus == Pane::RightTab && i == cursor;
        let commit_row = match item {
            GraphItem::Commit(r) => {
                let row = &app.graph.rows[*r];
                lines.push(render_row(
                    row,
                    app.graph.max_col,
                    focused,
                    app.config.graph_show_author,
                    app.config.graph_show_date,
                ));
                *r
            }
            GraphItem::File(k) => {
                let r = parent_commit_row(&items, i, &app.graph.rows);
                lines.push(file_row(app, *k, r, focused));
                r.unwrap_or(0)
            }
        };
        let next_is_file = matches!(items.get(i + 1), Some(GraphItem::File(_)));
        if !next_is_file && commit_row + 1 < app.graph.rows.len() {
            lines.push(connector_row(&app.graph.rows[commit_row], app.graph.max_col));
        }
    }
    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

fn parent_commit_row(items: &[GraphItem], from: usize, rows: &[GraphRow]) -> Option<usize> {
    (0..=from.min(items.len() - 1)).rev().find_map(|i| match items[i] {
        GraphItem::Commit(r) if r < rows.len() => Some(r),
        _ => None,
    })
}

fn file_row(app: &TuiApp, k: usize, commit_row: Option<usize>, focused: bool) -> Line<'static> {
    let mut spans = match commit_row {
        Some(r) => lane_spans(&app.graph.rows[r], app.graph.max_col),
        None => vec![Span::raw(" ".repeat(app.graph.max_col * 2 + 1))],
    };
    spans.push(Span::raw("   "));
    let Some(f) = app.commit_files.get(k) else {
        return Line::from(spans);
    };
    let mut style = Style::default();
    if focused {
        style = style.add_modifier(Modifier::REVERSED);
    }
    spans.push(Span::styled(
        format!("{} ", f.kind.marker()),
        style.fg(Color::Yellow),
    ));
    spans.push(Span::styled(f.path.clone(), style));
    Line::from(spans)
}

fn lane_spans(row: &GraphRow, max_col: usize) -> Vec<Span<'static>> {
    let mut cont: Vec<Option<usize>> = vec![None; max_col + 1];
    for seg in &row.segments {
        match *seg {
            Segment::Through { col, color } | Segment::NodeToBottom { col, color } => {
                cont[col] = Some(color)
            }
            Segment::TopToNode { .. } => {}
        }
    }
    let mut spans: Vec<Span> = Vec::new();
    for (col, color) in cont.iter().enumerate() {
        match color {
            Some(c) => spans.push(Span::styled("│", Style::default().fg(lane_color(*c)))),
            None => spans.push(Span::raw(" ")),
        }
        if col < max_col {
            spans.push(Span::raw(" "));
        }
    }
    spans
}

fn connector_row(row: &GraphRow, max_col: usize) -> Line<'static> {
    Line::from(lane_spans(row, max_col))
}

fn render_row(
    row: &GraphRow,
    max_col: usize,
    cursor: bool,
    show_author: bool,
    show_date: bool,
) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();

    let mut through: Vec<Option<usize>> = vec![None; max_col + 1];
    let mut top: Vec<Option<usize>> = vec![None; max_col + 1];
    let mut bottom: Vec<Option<usize>> = vec![None; max_col + 1];
    for seg in &row.segments {
        match *seg {
            Segment::Through { col, color } => through[col] = Some(color),
            Segment::TopToNode { col, color } => top[col] = Some(color),
            Segment::NodeToBottom { col, color } => bottom[col] = Some(color),
        }
    }

    let node = row.node_col;
    let diag_min = (0..=max_col)
        .filter(|&c| c == node || top[c].is_some() || bottom[c].is_some())
        .min()
        .unwrap_or(node);
    let diag_max = (0..=max_col)
        .filter(|&c| c == node || top[c].is_some() || bottom[c].is_some())
        .max()
        .unwrap_or(node);

    for col in 0..=max_col {
        let (ch, color) = if col == node {
            let ch = if row.is_uncommitted { '○' } else { '●' };
            (ch, Some(row.node_color))
        } else if let Some(c) = top[col] {
            (if col > node { '╯' } else { '╰' }, Some(c))
        } else if let Some(c) = bottom[col] {
            (if col > node { '╮' } else { '╭' }, Some(c))
        } else if let Some(c) = through[col] {
            ('│', Some(c))
        } else if col > diag_min && col < diag_max {
            ('─', Some(row.node_color))
        } else {
            (' ', None)
        };
        let gap = if col >= diag_min && col < diag_max {
            '─'
        } else {
            ' '
        };
        let style = match color {
            Some(c) => Style::default().fg(lane_color(c)),
            None => Style::default(),
        };
        spans.push(Span::styled(ch.to_string(), style));
        if col < max_col {
            spans.push(Span::styled(
                gap.to_string(),
                Style::default().fg(lane_color(row.node_color)),
            ));
        }
    }
    spans.push(Span::raw(" "));

    let text_style = if cursor {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };

    if row.is_uncommitted {
        spans.push(Span::styled(
            row.summary.clone(),
            text_style.fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ));
        return Line::from(spans);
    }

    spans.push(Span::styled(
        format!("{} ", row.short_id),
        text_style.fg(Color::DarkGray),
    ));
    for r in &row.refs {
        let color = match r.kind {
            RefKind::LocalBranch => Color::Green,
            RefKind::RemoteBranch => Color::Red,
            RefKind::Tag => Color::Yellow,
            RefKind::Stash => Color::Magenta,
            RefKind::DetachedHead => Color::Cyan,
        };
        let mut style = text_style.fg(color);
        if r.is_head {
            style = style.add_modifier(Modifier::BOLD);
        }
        spans.push(Span::styled(format!("({}) ", r.name), style));
    }
    let mut summary = text_style;
    if row.is_head {
        summary = summary.add_modifier(Modifier::BOLD);
    }
    spans.push(Span::styled(row.summary.clone(), summary));
    if show_author && !row.author.is_empty() {
        spans.push(Span::styled(
            format!("  {}", row.author),
            text_style.fg(Color::DarkGray),
        ));
    }
    if show_date && !row.date.is_empty() {
        spans.push(Span::styled(
            format!("  {}", row.date),
            text_style.fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}
