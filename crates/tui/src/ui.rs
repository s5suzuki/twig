use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, Paragraph, Tabs};
use twig_core::repo::RepoNode;

use crate::app::{Pane, Tab, TuiApp};

const FOCUS_FG: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &TuiApp) {
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
    let mut items: Vec<ListItem> = Vec::new();
    push_repo_items(&app.root, 0, app, &mut items);
    let list = List::new(items).block(pane_block("Repositories", app.focus == Pane::Sidebar));
    frame.render_widget(list, area);
}

fn push_repo_items(node: &RepoNode, depth: usize, app: &TuiApp, out: &mut Vec<ListItem>) {
    let selected = node.path == app.selected;
    let mut label = format!("{}{}", "  ".repeat(depth), node.name);
    if !node.initialized {
        label.push_str(" (uninit)");
    }
    if node.drifted {
        label.push_str(" *drift");
    }
    if node.dirty {
        label.push_str(" *dirty");
    }
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    out.push(ListItem::new(label).style(style));
    for child in &node.children {
        push_repo_items(child, depth + 1, app, out);
    }
}

fn draw_changes(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();
    if let Some(err) = &app.error {
        items.push(ListItem::new(err.clone()).style(Style::default().fg(Color::Red)));
    }
    items.push(section_header(format!("Staged ({})", app.staged.len())));
    for e in &app.staged {
        items.push(ListItem::new(format!(" {} {}", e.kind.marker(), e.path)));
    }
    items.push(section_header(format!("Changes ({})", app.unstaged.len())));
    for e in &app.unstaged {
        items.push(ListItem::new(format!(" {} {}", e.kind.marker(), e.path)));
    }
    let list = List::new(items).block(pane_block("Changes", app.focus == Pane::Changes));
    frame.render_widget(list, area);
}

fn section_header(text: String) -> ListItem<'static> {
    ListItem::new(text).style(Style::default().add_modifier(Modifier::BOLD))
}

fn draw_right(frame: &mut Frame, app: &TuiApp, area: Rect) {
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

    let placeholder = match app.active_tab {
        Tab::Graph => "(graph view: Phase 3)",
        Tab::Diff => "(diff view: Phase 3)",
    };
    frame.render_widget(
        Paragraph::new(Line::from(placeholder)).style(Style::default().fg(Color::DarkGray)),
        rows[1],
    );
}
