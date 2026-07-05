use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{SearchRow, TuiApp};
use crate::ui::FOCUS_FG;

const MATCH_FG: Color = Color::Yellow;

pub fn draw(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

    let status = if app.search.query.is_empty() {
        "/: search  (Enter/e: open in editor, r: replace all)".to_string()
    } else {
        format!(
            "\"{}\" — {} matches in {} files  (/: search  r: replace  Enter: editor)",
            app.search.query,
            app.search.match_count(),
            app.search.hits.len()
        )
    };
    frame.render_widget(
        Paragraph::new(Line::styled(status, Style::default().fg(Color::DarkGray))),
        parts[0],
    );

    let list = parts[1];
    let h = list.height as usize;
    app.search.view_rows = h;
    let rows = app.search.rows();
    if rows.is_empty() || h == 0 {
        return;
    }

    let cursor = app.search.cursor.min(rows.len() - 1);
    if cursor < app.search.scroll {
        app.search.scroll = cursor;
    }
    if cursor >= app.search.scroll + h {
        app.search.scroll = cursor + 1 - h;
    }
    app.search.scroll = app.search.scroll.min(rows.len() - 1);

    let mut lines: Vec<Line> = Vec::new();
    let end = (app.search.scroll + h).min(rows.len());
    for (i, row) in rows[app.search.scroll..end].iter().enumerate() {
        let focused = app.search.scroll + i == cursor;
        lines.push(render_row(app, row, focused));
    }
    frame.render_widget(Paragraph::new(lines), list);
}

fn render_row(app: &TuiApp, row: &SearchRow, focused: bool) -> Line<'static> {
    let cursor_style = if focused {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    match row {
        SearchRow::File(i) => {
            let f = &app.search.hits[*i];
            Line::from(Span::styled(
                format!("{} ({})", f.path, f.lines.len()),
                cursor_style.add_modifier(Modifier::BOLD).fg(FOCUS_FG),
            ))
        }
        SearchRow::Line(i, j) => {
            let l = &app.search.hits[*i].lines[*j];
            let mut spans = vec![Span::styled(
                format!("  {:>4}: ", l.line_no),
                cursor_style.fg(Color::DarkGray),
            )];
            let text = &l.text;
            let mut pos = 0usize;
            for (s, e) in &l.ranges {
                let (s, e) = (*s.min(&text.len()), *e.min(&text.len()));
                if s > pos
                    && let Some(head) = text.get(pos..s)
                {
                    spans.push(Span::styled(head.to_string(), cursor_style));
                }
                if let Some(m) = text.get(s..e) {
                    spans.push(Span::styled(
                        m.to_string(),
                        cursor_style.fg(MATCH_FG).add_modifier(Modifier::BOLD),
                    ));
                }
                pos = e;
            }
            if let Some(tail) = text.get(pos..) {
                spans.push(Span::styled(tail.to_string(), cursor_style));
            }
            Line::from(spans)
        }
    }
}
