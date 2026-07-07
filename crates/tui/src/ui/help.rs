use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use twit_core::keymap::{Action, Context};

use crate::app::{Pane, Tab, TuiApp};
use crate::ui::FOCUS_FG;

fn tui_supported(action: Action) -> bool {
    !matches!(
        action,
        Action::FocusUp
            | Action::FocusDown
            | Action::ToggleShell
            | Action::GraphContextMenu
    )
}

pub fn draw(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(
        "Keybindings — ? or Esc to close, j/k to scroll",
        Style::default().fg(FOCUS_FG).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));

    let focused = match (app.focus, app.active_tab) {
        (Pane::Sidebar, _) => Some(Context::Sidebar),
        (Pane::Changes, _) => Some(Context::Changes),
        (Pane::RightTab, Tab::Graph) => Some(Context::Graph),
        (Pane::RightTab, Tab::Diff) => Some(Context::Diff),
        (Pane::RightTab, Tab::Search) => None,
        (Pane::RightTab, Tab::Editor) => None,
    };
    if let Some(ctx) = focused {
        section(&mut lines, app, ctx);
    }
    section(&mut lines, app, Context::Global);
    extras(&mut lines);

    let h = area.height as usize;
    app.help_scroll = app.help_scroll.min(lines.len().saturating_sub(h));
    let visible: Vec<Line> = lines.into_iter().skip(app.help_scroll).take(h).collect();
    frame.render_widget(Paragraph::new(visible), area);
}

fn section(lines: &mut Vec<Line<'static>>, app: &TuiApp, ctx: Context) {
    lines.push(Line::styled(
        ctx.title().to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for e in app.keymap.help_for(ctx) {
        if !tui_supported(e.action) {
            continue;
        }
        lines.push(Line::raw(format!("  {:<18} {}", e.keys, e.desc)));
    }
    lines.push(Line::raw(""));
}

fn extras(lines: &mut Vec<Line<'static>>) {
    lines.push(Line::styled(
        "Other keys".to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for (keys, desc) in [
        ("Q / Ctrl+C", "Quit this pane"),
        ("?", "Toggle this help"),
        (",", "Open the settings overlay"),
        ("c / a / z", "Changes: commit / amend / stash"),
        ("Space (stash row)", "Changes: pop / apply / drop the stash"),
        ("n / N", "Diff: next / previous find match"),
        ("i / u", "Sidebar: initialize / update the submodule"),
        ("/ r Enter", "Search: query / replace all / open in editor"),
        ("C / A", "Continue / abort the in-progress rebase etc."),
    ] {
        lines.push(Line::raw(format!("  {keys:<18} {desc}")));
    }
    lines.push(Line::styled(
        "Keys are configurable in ~/.config/twig ([keys.*]).".to_string(),
        Style::default().fg(Color::DarkGray),
    ));
}
