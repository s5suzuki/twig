use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::app::{App, Pane};
use twig_core::repo::{FileNode, RepoNode};

enum RowKind {
    Repo { initialized: bool },
    Dir,
    File,
}

struct SideRow {
    path: PathBuf,
    depth: usize,
    kind: RowKind,
    expandable: bool,
    expanded: bool,
}

pub fn draw_tree(app: &mut App, ui: &mut egui::Ui) {
    ui.set_max_width(ui.available_width());

    let show_files = app.config.show_files;
    if show_files {
        ensure_file_cache(app);
    }

    let selected = app.selected.clone();
    let scroll = std::mem::take(&mut app.sidebar_scroll_pending);

    let mut rows: Vec<SideRow> = Vec::new();
    if let Some(root) = &app.root {
        flatten(root, 0, show_files, &app.file_cache, ui.ctx(), &mut rows);
    }

    let nav = sidebar_nav(app, ui, &rows);

    let cursor_path = rows.get(app.sidebar_cursor).map(|r| r.path.clone());

    let mut newly_selected: Option<PathBuf> = None;
    let mut file_to_open: Option<PathBuf> = None;
    let mut sub_action: Option<SubAction> = None;
    match nav {
        Some(Nav::SelectRepo(p)) => newly_selected = Some(p),
        Some(Nav::OpenFile(p)) => file_to_open = Some(p),
        Some(Nav::SetRepoExpanded(path, val)) => {
            if let Some(root) = &mut app.root {
                set_expanded(root, &path, val);
            }
        }
        Some(Nav::SetDirExpanded(path, val)) => set_file_dir_open(ui.ctx(), &path, val),
        None => {}
    }

    egui::ScrollArea::vertical()
        .id_salt("sidebar")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let cache = &app.file_cache;
            if let Some(root) = &mut app.root {
                draw_node(
                    root,
                    &selected,
                    cursor_path.as_deref(),
                    None,
                    show_files,
                    cache,
                    scroll,
                    ui,
                    &mut newly_selected,
                    &mut file_to_open,
                    &mut sub_action,
                );
            } else {
                ui.weak("(no repository loaded)");
            }
        });

    if let Some(path) = newly_selected {
        app.select_repo(path);
    }
    if let Some(path) = file_to_open {
        app.open_abs_in_editor(path);
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

fn ensure_file_cache(app: &mut App) {
    let mut want: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    if let Some(root) = &app.root {
        collect_repos(root, &mut want);
    }
    for (repo, subs) in want {
        app.file_cache
            .entry(repo)
            .or_insert_with_key(|repo| twig_core::repo::list_files(repo, &subs));
    }
}

fn collect_repos(node: &RepoNode, out: &mut Vec<(PathBuf, Vec<PathBuf>)>) {
    if !node.expanded || !node.initialized {
        return;
    }
    let subs: Vec<PathBuf> = node
        .children
        .iter()
        .filter_map(|c| c.path.strip_prefix(&node.path).ok().map(Path::to_path_buf))
        .collect();
    out.push((node.path.clone(), subs));
    for c in &node.children {
        collect_repos(c, out);
    }
}

enum Nav {
    SelectRepo(PathBuf),
    OpenFile(PathBuf),
    SetRepoExpanded(PathBuf, bool),
    SetDirExpanded(PathBuf, bool),
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
    let before = app.sidebar_cursor;

    if has(Cmd::SidebarBottom) {
        app.sidebar_cursor = last;
    }
    if has(Cmd::SidebarTop) {
        app.sidebar_cursor = 0;
    }
    if has(Cmd::SidebarDown) {
        app.sidebar_cursor = (app.sidebar_cursor + 1).min(last);
    }
    if has(Cmd::SidebarUp) {
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
    if has(Cmd::SidebarSelect) {
        nav = activate(cur);
    }
    if has(Cmd::SidebarExpand) {
        if cur.expandable && !cur.expanded {
            nav = Some(expand(cur, true));
        } else if cur.expandable && cur.expanded {
            if app.sidebar_cursor < last {
                app.sidebar_cursor += 1;
            }
        } else {
            nav = activate(cur);
        }
    }
    if has(Cmd::SidebarCollapse) {
        if cur.expandable && cur.expanded {
            nav = Some(expand(cur, false));
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

    if app.sidebar_cursor != before {
        app.sidebar_scroll_pending = true;
    }
    nav
}

fn activate(row: &SideRow) -> Option<Nav> {
    match row.kind {
        RowKind::Repo { initialized } if initialized => Some(Nav::SelectRepo(row.path.clone())),
        RowKind::File => Some(Nav::OpenFile(row.path.clone())),
        RowKind::Dir => Some(Nav::SetDirExpanded(row.path.clone(), !row.expanded)),
        RowKind::Repo { .. } => None,
    }
}

fn expand(row: &SideRow, val: bool) -> Nav {
    match row.kind {
        RowKind::Dir => Nav::SetDirExpanded(row.path.clone(), val),
        _ => Nav::SetRepoExpanded(row.path.clone(), val),
    }
}

fn flatten(
    node: &RepoNode,
    depth: usize,
    show_files: bool,
    cache: &HashMap<PathBuf, Vec<FileNode>>,
    ctx: &egui::Context,
    out: &mut Vec<SideRow>,
) {
    let files = show_files.then(|| cache.get(&node.path)).flatten();
    let has_files = files.is_some_and(|f| !f.is_empty());
    out.push(SideRow {
        path: node.path.clone(),
        depth,
        kind: RowKind::Repo {
            initialized: node.initialized,
        },
        expandable: !node.children.is_empty() || has_files,
        expanded: node.expanded,
    });
    if node.expanded {
        for c in &node.children {
            flatten(c, depth + 1, show_files, cache, ctx, out);
        }
        if let Some(files) = files {
            for f in files {
                flatten_file(&node.path, f, depth + 1, ctx, out);
            }
        }
    }
}

fn flatten_file(
    repo: &Path,
    node: &FileNode,
    depth: usize,
    ctx: &egui::Context,
    out: &mut Vec<SideRow>,
) {
    let path = repo.join(&node.rel);
    if node.is_dir {
        let expanded = file_dir_open(ctx, &path);
        out.push(SideRow {
            path: path.clone(),
            depth,
            kind: RowKind::Dir,
            expandable: true,
            expanded,
        });
        if expanded {
            for c in &node.children {
                flatten_file(repo, c, depth + 1, ctx, out);
            }
        }
    } else {
        out.push(SideRow {
            path,
            depth,
            kind: RowKind::File,
            expandable: false,
            expanded: false,
        });
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

fn file_dir_id(path: &Path) -> egui::Id {
    egui::Id::new(("side_dir", path))
}
fn file_dir_open(ctx: &egui::Context, path: &Path) -> bool {
    ctx.data(|d| d.get_temp::<bool>(file_dir_id(path)).unwrap_or(false))
}
fn set_file_dir_open(ctx: &egui::Context, path: &Path, open: bool) {
    ctx.data_mut(|d| d.insert_temp(file_dir_id(path), open));
}

fn cursor_frame(ui: &egui::Ui, resp: &egui::Response, is_cursor: bool, scroll: bool) {
    if !is_cursor {
        return;
    }
    ui.painter().rect_stroke(
        resp.rect.expand(1.0),
        2.0,
        egui::Stroke::new(1.0, ui.visuals().selection.bg_fill),
        egui::StrokeKind::Inside,
    );
    if scroll {
        ui.scroll_to_rect(resp.rect, None);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_node(
    node: &mut RepoNode,
    selected: &Path,
    cursor: Option<&Path>,
    parent: Option<&Path>,
    show_files: bool,
    cache: &HashMap<PathBuf, Vec<FileNode>>,
    scroll: bool,
    ui: &mut egui::Ui,
    out: &mut Option<PathBuf>,
    file_out: &mut Option<PathBuf>,
    sub_action: &mut Option<SubAction>,
) {
    let is_cursor = cursor == Some(node.path.as_path());
    let files = show_files.then(|| cache.get(&node.path)).flatten();
    let has_files = files.is_some_and(|f| !f.is_empty());
    let resp = ui.horizontal(|ui| {
        if node.children.is_empty() && !has_files {
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

    cursor_frame(ui, &resp.response, is_cursor, scroll);

    if node.expanded && (!node.children.is_empty() || has_files) {
        let parent_path = node.path.clone();
        let repo_path = node.path.clone();
        ui.indent(node.path.clone(), |ui| {
            for child in &mut node.children {
                draw_node(
                    child,
                    selected,
                    cursor,
                    Some(&parent_path),
                    show_files,
                    cache,
                    scroll,
                    ui,
                    out,
                    file_out,
                    sub_action,
                );
            }
            if let Some(files) = files {
                for f in files {
                    draw_file_node(&repo_path, f, cursor, scroll, ui, file_out);
                }
            }
        });
    }
}

fn draw_file_node(
    repo: &Path,
    node: &FileNode,
    cursor: Option<&Path>,
    scroll: bool,
    ui: &mut egui::Ui,
    file_out: &mut Option<PathBuf>,
) {
    let path = repo.join(&node.rel);
    let is_cursor = cursor == Some(path.as_path());
    if node.is_dir {
        let open = file_dir_open(ui.ctx(), &path);
        let resp = ui.horizontal(|ui| {
            let arrow = if open { "▼" } else { "▶" };
            if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                set_file_dir_open(ui.ctx(), &path, !open);
            }
            let b = ui.add(
                egui::Button::selectable(false, format!("\u{f07b}  {}", node.name)).truncate(),
            );
            if b.clicked() {
                set_file_dir_open(ui.ctx(), &path, !open);
            }
            b
        });
        cursor_frame(ui, &resp.response, is_cursor, scroll);
        if open {
            ui.indent(path.clone(), |ui| {
                for c in &node.children {
                    draw_file_node(repo, c, cursor, scroll, ui, file_out);
                }
            });
        }
    } else {
        let resp = ui.horizontal(|ui| {
            ui.add_space(14.0);
            let b = ui.add(
                egui::Button::selectable(false, format!("\u{f15b}  {}", node.name)).truncate(),
            );
            if b.clicked() {
                *file_out = Some(path.clone());
            }
            b
        });
        cursor_frame(ui, &resp.response, is_cursor, scroll);
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
