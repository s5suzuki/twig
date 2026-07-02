use std::path::{Path, PathBuf};

use crate::app::{App, Pane};
use crate::repo::RepoNode;

struct SideRow {
    path: PathBuf,
    depth: usize,
    expandable: bool,
    expanded: bool,
    initialized: bool,
}

pub fn draw_tree(app: &mut App, ui: &mut egui::Ui) {
    ui.set_max_width(ui.available_width());

    let selected = app.selected.clone();

    let mut rows: Vec<SideRow> = Vec::new();
    if let Some(root) = &app.root {
        flatten(root, 0, &mut rows);
    }

    let nav = sidebar_nav(app, ui, &rows);

    let cursor_path = rows.get(app.sidebar_cursor).map(|r| r.path.clone());

    let mut newly_selected: Option<PathBuf> = None;
    let mut sub_action: Option<SubAction> = None;
    match nav {
        Some(Nav::Select(p)) => newly_selected = Some(p),
        Some(Nav::SetExpanded(path, val)) => {
            if let Some(root) = &mut app.root {
                set_expanded(root, &path, val);
            }
        }
        None => {}
    }

    if let Some(root) = &mut app.root {
        draw_node(
            root,
            &selected,
            cursor_path.as_deref(),
            None,
            ui,
            &mut newly_selected,
            &mut sub_action,
        );
    } else {
        ui.weak("(no repository loaded)");
    }

    if let Some(path) = newly_selected {
        app.select_repo(path);
    }
    match sub_action {
        Some(SubAction::Init(parent, name)) => {
            let ctx = ui.ctx().clone();
            app.submodule_init(&ctx, parent, name);
        }
        Some(SubAction::Update(parent, name)) => {
            let ctx = ui.ctx().clone();
            app.submodule_update(&ctx, parent, name);
        }
        None => {}
    }
}

enum Nav {
    Select(PathBuf),
    SetExpanded(PathBuf, bool),
}

enum SubAction {
    Init(PathBuf, String),
    Update(PathBuf, String),
}

fn sidebar_nav(app: &mut App, ui: &mut egui::Ui, rows: &[SideRow]) -> Option<Nav> {
    if rows.is_empty() {
        app.sidebar_cursor = 0;
        return None;
    }
    let last = rows.len() - 1;
    if app.sidebar_cursor > last {
        app.sidebar_cursor = last;
    }
    if app.focus != Pane::Sidebar
        || app.help_open
        || app.any_modal_open()
        || ui.ctx().memory(|m| m.focused().is_some())
    {
        return None;
    }

    use crate::keys::{Action as Cmd, Context};
    let acts = app
        .keymap
        .poll(ui, Context::Sidebar, &mut app.pending_prefix, |_| true);
    let has = |a: Cmd| acts.contains(&a);
    let go_top = has(Cmd::SidebarTop);
    let to_bottom = has(Cmd::SidebarBottom);
    let j = has(Cmd::SidebarDown);
    let k = has(Cmd::SidebarUp);

    if to_bottom {
        app.sidebar_cursor = last;
    }
    if go_top {
        app.sidebar_cursor = 0;
    }
    if j {
        app.sidebar_cursor = (app.sidebar_cursor + 1).min(last);
    }
    if k {
        app.sidebar_cursor = app.sidebar_cursor.saturating_sub(1);
    }
    if has(Cmd::SidebarHalfPageDown) {
        app.sidebar_cursor = (app.sidebar_cursor + crate::app::LIST_PAGE).min(last);
    }
    if has(Cmd::SidebarHalfPageUp) {
        app.sidebar_cursor = app.sidebar_cursor.saturating_sub(crate::app::LIST_PAGE);
    }

    let cur = &rows[app.sidebar_cursor];
    let mut nav = None;
    if has(Cmd::SidebarSelect) && cur.initialized {
        nav = Some(Nav::Select(cur.path.clone()));
    }
    if has(Cmd::SidebarExpand) {
        if cur.expandable && !cur.expanded {
            nav = Some(Nav::SetExpanded(cur.path.clone(), true));
        } else if cur.expandable && cur.expanded {
            if app.sidebar_cursor < last {
                app.sidebar_cursor += 1;
            }
        } else if cur.initialized {
            nav = Some(Nav::Select(cur.path.clone()));
        }
    }
    if has(Cmd::SidebarCollapse) {
        if cur.expandable && cur.expanded {
            nav = Some(Nav::SetExpanded(cur.path.clone(), false));
        } else {
            let depth = cur.depth;
            if depth > 0
                && let Some(p) = (0..app.sidebar_cursor)
                    .rev()
                    .find(|&i| rows[i].depth < depth)
                {
                    app.sidebar_cursor = p;
                }
        }
    }
    nav
}

fn flatten(node: &RepoNode, depth: usize, out: &mut Vec<SideRow>) {
    out.push(SideRow {
        path: node.path.clone(),
        depth,
        expandable: !node.children.is_empty(),
        expanded: node.expanded,
        initialized: node.initialized,
    });
    if node.expanded {
        for c in &node.children {
            flatten(c, depth + 1, out);
        }
    }
}

fn set_expanded(node: &mut RepoNode, path: &Path, val: bool) -> bool {
    if node.path == path {
        node.expanded = val;
        return true;
    }
    for c in &mut node.children {
        if set_expanded(c, path, val) {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn draw_node(
    node: &mut RepoNode,
    selected: &Path,
    cursor: Option<&Path>,
    parent: Option<&Path>,
    ui: &mut egui::Ui,
    out: &mut Option<PathBuf>,
    sub_action: &mut Option<SubAction>,
) {
    let is_cursor = cursor == Some(node.path.as_path());
    let resp = ui.horizontal(|ui| {
        if node.children.is_empty() {
            ui.add_space(14.0);
        } else {
            let arrow = if node.expanded { "▼" } else { "▶" };
            if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                node.expanded = !node.expanded;
            }
        }

        let label = if !node.initialized {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{} (uninitialized)", node.name)).weak(),
                )
                .truncate()
                .sense(egui::Sense::click()),
            )
        } else {
            let is_sel = node.path == selected;
            let b = ui.add(egui::Button::selectable(is_sel, &node.name).truncate());
            if b.clicked() {
                *out = Some(node.path.clone());
            }
            b
        };
        draw_badges(node, ui);
        label
    });

    if let Some(parent) = parent {
        submodule_menu(&resp.inner, node, parent, sub_action);
    }

    if is_cursor {
        ui.painter().rect_stroke(
            resp.response.rect.expand(1.0),
            2.0,
            egui::Stroke::new(1.0, ui.visuals().selection.bg_fill),
            egui::StrokeKind::Inside,
        );
    }

    if node.expanded && !node.children.is_empty() {
        let parent_path = node.path.clone();
        ui.indent(node.path.clone(), |ui| {
            for child in &mut node.children {
                draw_node(
                    child,
                    selected,
                    cursor,
                    Some(&parent_path),
                    ui,
                    out,
                    sub_action,
                );
            }
        });
    }
}

fn draw_badges(node: &RepoNode, ui: &mut egui::Ui) {
    if node.drifted {
        ui.add(egui::Label::new(
            egui::RichText::new("\u{f062}")
                .color(egui::Color32::from_rgb(0xff, 0xb8, 0x6c))
                .small(),
        ))
        .on_hover_text("Checked out at a commit the parent does not record");
    }
    if node.dirty {
        ui.add(egui::Label::new(
            egui::RichText::new("\u{f111}")
                .color(egui::Color32::from_rgb(0xe6, 0xd8, 0x6b))
                .small(),
        ))
        .on_hover_text("Uncommitted changes inside the submodule");
    }
}

fn submodule_menu(
    resp: &egui::Response,
    node: &RepoNode,
    parent: &Path,
    sub_action: &mut Option<SubAction>,
) {
    resp.context_menu(|ui| {
        if node.initialized {
            if ui.button("\u{f021}  Update").clicked() {
                *sub_action = Some(SubAction::Update(parent.to_path_buf(), node.name.clone()));
                ui.close();
            }
        } else if ui.button("\u{f019}  Initialize").clicked() {
            *sub_action = Some(SubAction::Init(parent.to_path_buf(), node.name.clone()));
            ui.close();
        }
    });
}
