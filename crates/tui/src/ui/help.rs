use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use twit_core::keymap::{Action, Context};

use crate::app::{Pane, Tab, TuiApp};
use crate::ui::{FOCUS_FG, wrap_plain};

fn tui_supported(action: Action) -> bool {
    !matches!(
        action,
        Action::FocusUp | Action::FocusDown | Action::ToggleShell | Action::GraphContextMenu
    )
}

pub fn draw(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let width = area.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    push_wrapped(
        &mut lines,
        width,
        "Keybindings — ? or Esc to close, j/k to scroll",
        Style::default().fg(FOCUS_FG).add_modifier(Modifier::BOLD),
    );
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
        section(&mut lines, app, ctx, width);
    }
    section(&mut lines, app, Context::Global, width);
    extras(&mut lines, width);

    let h = area.height as usize;
    app.help_scroll = app.help_scroll.min(lines.len().saturating_sub(h));
    let visible: Vec<Line> = lines.into_iter().skip(app.help_scroll).take(h).collect();
    frame.render_widget(Paragraph::new(visible), area);
}

fn push_wrapped(lines: &mut Vec<Line<'static>>, width: usize, text: &str, style: Style) {
    for chunk in wrap_plain(text, width) {
        lines.push(Line::styled(chunk, style));
    }
}

fn push_kv(lines: &mut Vec<Line<'static>>, width: usize, keys: &str, desc: &str) {
    let prefix = format!("  {keys:<18} ");
    let indent = prefix.chars().count();
    if width.saturating_sub(indent) < 6 {
        push_wrapped(lines, width, &format!("{prefix}{desc}"), Style::default());
        return;
    }
    let pad = " ".repeat(indent);
    for (i, chunk) in wrap_plain(desc, width - indent).into_iter().enumerate() {
        let row = if i == 0 {
            format!("{prefix}{chunk}")
        } else {
            format!("{pad}{chunk}")
        };
        lines.push(Line::raw(row));
    }
}

fn section(lines: &mut Vec<Line<'static>>, app: &TuiApp, ctx: Context, width: usize) {
    push_wrapped(
        lines,
        width,
        ctx.title(),
        Style::default().add_modifier(Modifier::BOLD),
    );
    for e in app.keymap.help_for(ctx) {
        if !tui_supported(e.action) {
            continue;
        }
        push_kv(lines, width, &e.keys, &e.desc);
    }
    lines.push(Line::raw(""));
}

fn extras(lines: &mut Vec<Line<'static>>, width: usize) {
    push_wrapped(
        lines,
        width,
        "Other keys",
        Style::default().add_modifier(Modifier::BOLD),
    );
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
        push_kv(lines, width, keys, desc);
    }
    push_wrapped(
        lines,
        width,
        "Keys are configurable in ~/.config/twig ([keys.*]).",
        Style::default().fg(Color::DarkGray),
    );
}
