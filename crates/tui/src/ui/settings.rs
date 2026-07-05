use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::TuiApp;
use crate::ui::FOCUS_FG;

pub fn draw(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(
        "Settings — , or Esc to close, j/k to move, Enter to edit/toggle",
        Style::default().fg(FOCUS_FG).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));

    for (i, (label, value)) in app.settings_rows().into_iter().enumerate() {
        let mut style = Style::default();
        if i == app.settings_cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        lines.push(Line::styled(format!("  {label:<22} {value}"), style));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Saved to ~/.config/twig/config.toml (shared with the GUI).",
        Style::default().fg(Color::DarkGray),
    ));

    let h = area.height as usize;
    let visible: Vec<Line> = lines.into_iter().take(h).collect();
    frame.render_widget(Paragraph::new(visible), area);
}
