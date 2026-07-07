use super::*;

pub(super) enum Action {
    Select(String, bool),
    Stage(Vec<String>),
    Unstage(Vec<String>),
    OpenEditor(String),
    RequestDiscard(DiscardReq),
}

pub(super) struct CommitGuide {
    subject_len: usize,
    line2_nonblank: bool,
    body_over_72: bool,
}

pub(super) fn commit_guide(msg: &str) -> CommitGuide {
    let mut lines = msg.split('\n');
    let subject_len = lines.next().unwrap_or("").chars().count();
    let line2_nonblank = lines.next().is_some_and(|l| !l.trim().is_empty());
    let body_over_72 = msg.split('\n').skip(2).any(|l| l.chars().count() > 72);
    CommitGuide {
        subject_len,
        line2_nonblank,
        body_over_72,
    }
}

pub(super) fn commit_guide_row(app: &App, ui: &mut egui::Ui) {
    if app.commit_msg.is_empty() {
        return;
    }
    let g = commit_guide(&app.commit_msg);
    let (warn, error) = if app.config.theme == Theme::CatppuccinMocha {
        (
            crate::theme::c32(Accent::Yellow.rgb()),
            crate::theme::c32(Accent::Red.rgb()),
        )
    } else {
        (ui.visuals().warn_fg_color, ui.visuals().error_fg_color)
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let count_color = if g.subject_len > 72 {
            error
        } else if g.subject_len > 50 {
            warn
        } else {
            ui.visuals().weak_text_color()
        };
        ui.add(
            egui::Label::new(
                egui::RichText::new(g.subject_len.to_string())
                    .small()
                    .color(count_color),
            )
            .selectable(false),
        )
        .on_hover_text("Subject length (50/72 convention)");
        if g.line2_nonblank {
            ui.add(
                egui::Label::new(egui::RichText::new("\u{f071}").small().color(warn))
                    .selectable(false),
            )
            .on_hover_text("Line 2 should be blank to separate subject from body");
        }
        if g.body_over_72 {
            ui.add(
                egui::Label::new(egui::RichText::new("72+").small().color(warn)).selectable(false),
            )
            .on_hover_text("Body has lines longer than 72 columns");
        }
    });
}

pub(super) enum StashAction {
    Pop(usize),
    Apply(usize),
    Drop(usize),
}

pub(super) fn draw_stashes(app: &App, ui: &mut egui::Ui) -> Option<StashAction> {
    if app.stashes.is_empty() {
        return None;
    }
    let mut action = None;
    egui::CollapsingHeader::new(format!("\u{f187}  Stashes ({})", app.stashes.len()))
        .id_salt("stashes")
        .default_open(false)
        .show(ui, |ui| {
            for s in &app.stashes {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("\u{f0ab}")
                        .on_hover_text("Pop (apply + drop)")
                        .clicked()
                    {
                        action = Some(StashAction::Pop(s.index));
                    }
                    if ui
                        .small_button("\u{f0c5}")
                        .on_hover_text("Apply (keep)")
                        .clicked()
                    {
                        action = Some(StashAction::Apply(s.index));
                    }
                    if ui.small_button("\u{f1f8}").on_hover_text("Drop").clicked() {
                        action = Some(StashAction::Drop(s.index));
                    }
                    ui.add(
                        egui::Label::new(format!("stash@{{{}}}: {}", s.index, s.message))
                            .truncate(),
                    );
                });
            }
        });
    action
}

#[derive(Default)]
pub(super) struct TreeDir {
    dirs: BTreeMap<String, TreeDir>,
    files: Vec<Leaf>,
}

pub(super) struct Leaf {
    name: String,
    path: String,
    old_path: Option<String>,
    kind: StatusKind,
}

pub(super) fn build_tree(entries: &[StatusEntry]) -> TreeDir {
    let mut root = TreeDir::default();
    for e in entries {
        let parts: Vec<&str> = e.path.split('/').collect();
        let (dirs, file) = parts.split_at(parts.len() - 1);
        let mut cur = &mut root;
        for d in dirs {
            cur = cur.dirs.entry((*d).to_string()).or_default();
        }
        cur.files.push(Leaf {
            name: file[0].to_string(),
            path: e.path.clone(),
            old_path: e.old_path.clone(),
            kind: e.kind,
        });
    }
    root
}

pub(super) struct NavRow {
    depth: usize,
    staged: bool,
    kind: NavKind,
}

pub(super) enum NavKind {
    Group {
        open: bool,
        count: usize,
        paths: Vec<String>,
    },
    Dir {
        salt: String,
        name: String,
        open: bool,
        paths: Vec<String>,
    },
    File {
        name: String,
        path: String,
        old_path: Option<String>,
        kind: StatusKind,
    },
}

pub(super) fn dir_id(salt: &str) -> egui::Id {
    egui::Id::new(("nav_dir", salt))
}

pub(super) fn dir_label(salt: &str) -> String {
    salt.split_once('/')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| salt.to_string())
}

pub(super) fn dir_open(ctx: &egui::Context, salt: &str) -> bool {
    ctx.data(|d| d.get_temp::<bool>(dir_id(salt)).unwrap_or(true))
}

pub(super) fn set_dir_open(ctx: &egui::Context, salt: &str, open: bool) {
    ctx.data_mut(|d| d.insert_temp(dir_id(salt), open));
}

pub(super) fn group_id(staged: bool) -> egui::Id {
    egui::Id::new(if staged {
        "nav_grp_staged"
    } else {
        "nav_grp_unstaged"
    })
}

pub(super) fn group_open(ctx: &egui::Context, staged: bool) -> bool {
    ctx.data(|d| d.get_temp::<bool>(group_id(staged)).unwrap_or(true))
}

pub(super) fn set_group_open(ctx: &egui::Context, staged: bool, open: bool) {
    ctx.data_mut(|d| d.insert_temp(group_id(staged), open));
}

pub(super) fn build_nav_rows(
    ctx: &egui::Context,
    dir: &TreeDir,
    staged: bool,
    salt: &str,
    depth: usize,
    out: &mut Vec<NavRow>,
) {
    for (name, sub) in &dir.dirs {
        let child = format!("{salt}/{name}");
        let open = dir_open(ctx, &child);
        out.push(NavRow {
            depth,
            staged,
            kind: NavKind::Dir {
                salt: child.clone(),
                name: name.clone(),
                open,
                paths: collect_paths(sub),
            },
        });
        if open {
            build_nav_rows(ctx, sub, staged, &child, depth + 1, out);
        }
    }
    for leaf in &dir.files {
        out.push(NavRow {
            depth,
            staged,
            kind: NavKind::File {
                name: leaf.name.clone(),
                path: leaf.path.clone(),
                old_path: leaf.old_path.clone(),
                kind: leaf.kind,
            },
        });
    }
}

pub(super) fn nav_paths(row: &NavRow) -> Vec<String> {
    match &row.kind {
        NavKind::File { path, old_path, .. } => file_paths(path, old_path.as_deref()),
        NavKind::Dir { paths, .. } => paths.clone(),
        NavKind::Group { paths, .. } => paths.clone(),
    }
}

pub(super) fn file_paths(path: &str, old_path: Option<&str>) -> Vec<String> {
    match old_path {
        Some(old) if old != path => vec![old.to_string(), path.to_string()],
        _ => vec![path.to_string()],
    }
}

pub(super) fn draw_changes(app: &mut App, ui: &mut egui::Ui) -> Option<Action> {
    let staged_tree = build_tree(&app.staged);
    let unstaged_tree = build_tree(&app.unstaged);
    let ctx = ui.ctx().clone();

    let mut rows: Vec<NavRow> = Vec::new();
    let staged_open = group_open(&ctx, true);
    rows.push(NavRow {
        depth: 0,
        staged: true,
        kind: NavKind::Group {
            open: staged_open,
            count: app.staged.len(),
            paths: collect_paths(&staged_tree),
        },
    });
    if staged_open {
        build_nav_rows(&ctx, &staged_tree, true, "s", 1, &mut rows);
    }
    let unstaged_open = group_open(&ctx, false);
    rows.push(NavRow {
        depth: 0,
        staged: false,
        kind: NavKind::Group {
            open: unstaged_open,
            count: app.unstaged.len(),
            paths: collect_paths(&unstaged_tree),
        },
    });
    if unstaged_open {
        build_nav_rows(&ctx, &unstaged_tree, false, "u", 1, &mut rows);
    }

    let mut action = changes_nav(app, ui, &rows);

    let sel = app.selected_file.clone();
    let cursor = app.changes_cursor;
    let scroll = std::mem::take(&mut app.changes_scroll_pending);
    render_rows(ui, &rows, 0, cursor, scroll, sel.as_ref(), &mut action);
    action
}

pub(super) fn changes_nav(app: &mut App, ui: &mut egui::Ui, rows: &[NavRow]) -> Option<Action> {
    if rows.is_empty() {
        app.changes_cursor = 0;
        return None;
    }
    let last = rows.len() - 1;
    if app.changes_cursor > last {
        app.changes_cursor = last;
    }
    if app.focus != Pane::Changes
        || app.help_open
        || app.any_modal_open()
        || ui.ctx().memory(|m| m.focused().is_some())
    {
        return None;
    }

    use crate::keys::{Action as Cmd, Context, KeymapPoll};
    let acts = app
        .keymap
        .poll(ui, Context::Changes, &mut app.pending_prefix, |_| true);
    let has = |a: Cmd| acts.contains(&a);
    let go_top = has(Cmd::ChangesTop);
    let to_bottom = has(Cmd::ChangesBottom);
    let j = has(Cmd::ChangesDown);
    let k = has(Cmd::ChangesUp);
    let h = has(Cmd::ChangesCollapse);
    let l = has(Cmd::ChangesExpand);
    let enter = has(Cmd::ChangesActivate);
    let space = has(Cmd::ChangesStageToggle);
    let e = has(Cmd::ChangesEdit);
    let d = has(Cmd::ChangesDiscard);

    let before = app.changes_cursor;
    if to_bottom {
        app.changes_cursor = last;
    }
    if go_top {
        app.changes_cursor = 0;
    }
    if j {
        app.changes_cursor = (app.changes_cursor + 1).min(last);
    }
    if k {
        app.changes_cursor = app.changes_cursor.saturating_sub(1);
    }
    if has(Cmd::ChangesHalfPageDown) {
        app.changes_cursor = (app.changes_cursor + crate::app::LIST_PAGE).min(last);
    }
    if has(Cmd::ChangesHalfPageUp) {
        app.changes_cursor = app.changes_cursor.saturating_sub(crate::app::LIST_PAGE);
    }

    let mut action = None;
    let cur = &rows[app.changes_cursor];

    let mut open_diff: Option<(String, bool)> = None;
    if enter {
        match &cur.kind {
            NavKind::File { path, .. } => open_diff = Some((path.clone(), cur.staged)),
            NavKind::Dir { salt, open, .. } => set_dir_open(ui.ctx(), salt, !*open),
            NavKind::Group { open, .. } => set_group_open(ui.ctx(), cur.staged, !*open),
        }
    }
    if l {
        match &cur.kind {
            NavKind::Dir { salt, open, .. } => {
                if !*open {
                    set_dir_open(ui.ctx(), salt, true);
                } else if app.changes_cursor < last {
                    app.changes_cursor += 1;
                }
            }
            NavKind::Group { open, .. } => {
                if !*open {
                    set_group_open(ui.ctx(), cur.staged, true);
                } else if app.changes_cursor < last {
                    app.changes_cursor += 1;
                }
            }
            NavKind::File { path, .. } => open_diff = Some((path.clone(), cur.staged)),
        }
    }
    if let Some((path, staged)) = open_diff {
        app.focus = Pane::RightTab;
        action = Some(Action::Select(path, staged));
    }
    if h {
        match &cur.kind {
            NavKind::Dir { salt, open, .. } if *open => set_dir_open(ui.ctx(), salt, false),
            NavKind::Group { open, .. } if *open => set_group_open(ui.ctx(), cur.staged, false),
            _ => {
                let depth = cur.depth;
                if depth > 0
                    && let Some(p) = (0..app.changes_cursor).rev().find(|&i| {
                        rows[i].depth < depth
                            && matches!(rows[i].kind, NavKind::Dir { .. } | NavKind::Group { .. })
                    })
                {
                    app.changes_cursor = p;
                }
            }
        }
    }
    if space {
        action = Some(if cur.staged {
            Action::Unstage(nav_paths(cur))
        } else {
            Action::Stage(nav_paths(cur))
        });
    }
    if e && let NavKind::File { path, .. } = &cur.kind {
        app.focus = Pane::RightTab;
        action = Some(Action::OpenEditor(path.clone()));
    }
    if d && !cur.staged {
        match &cur.kind {
            NavKind::File { path, old_path, .. } => {
                action = Some(Action::RequestDiscard(DiscardReq {
                    paths: file_paths(path, old_path.as_deref()),
                    label: path.clone(),
                }));
            }
            NavKind::Dir { salt, paths, .. } => {
                action = Some(Action::RequestDiscard(DiscardReq {
                    paths: paths.clone(),
                    label: dir_label(salt),
                }));
            }
            NavKind::Group { paths, .. } if !paths.is_empty() => {
                action = Some(Action::RequestDiscard(DiscardReq {
                    paths: paths.clone(),
                    label: "all files".to_string(),
                }));
            }
            NavKind::Group { .. } => {}
        }
    }
    if app.changes_cursor != before {
        app.changes_scroll_pending = true;
    }
    action
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_rows(
    ui: &mut egui::Ui,
    rows: &[NavRow],
    base: usize,
    cursor: usize,
    scroll: bool,
    sel: Option<&(String, bool)>,
    action: &mut Option<Action>,
) {
    for (i, row) in rows.iter().enumerate() {
        let is_cursor = base + i == cursor;
        let rect = match &row.kind {
            NavKind::Group { open, count, paths } => {
                render_group_row(ui, row.staged, *open, *count, paths, is_cursor, action)
            }
            NavKind::Dir {
                salt,
                name,
                open,
                paths,
            } => render_dir_row(
                ui, row.staged, salt, name, *open, paths, row.depth, is_cursor, action,
            ),
            NavKind::File {
                name,
                path,
                old_path,
                kind,
            } => render_file_row(
                ui,
                row.staged,
                name,
                path,
                old_path.as_deref(),
                *kind,
                row.depth,
                is_cursor,
                sel,
                action,
            ),
        };
        if is_cursor && scroll {
            ui.scroll_to_rect(rect, None);
        }
    }
}

pub(super) fn render_group_row(
    ui: &mut egui::Ui,
    staged: bool,
    open: bool,
    count: usize,
    paths: &[String],
    is_cursor: bool,
    action: &mut Option<Action>,
) -> egui::Rect {
    let w = ui.available_width().max(40.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 20.0), egui::Sense::click());
    let hovered = ui.rect_contains_pointer(rect);

    if is_cursor {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().selection.bg_fill);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let text_color = ui.visuals().strong_text_color();
    let weak = egui::Color32::from_gray(150);
    ui.painter().text(
        egui::pos2(rect.left() + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        if open { "▼" } else { "▶" },
        egui::FontId::proportional(10.0),
        weak,
    );
    let title = if staged {
        format!("Staged Changes ({count})")
    } else {
        format!("Changes ({count})")
    };
    ui.painter().text(
        egui::pos2(rect.left() + 20.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(14.0),
        text_color,
    );

    let mut btn_clicked = false;
    if !staged && !paths.is_empty() {
        let del_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 2.0 * BTN_W - 2.0, rect.center().y - 9.0),
            egui::vec2(BTN_W, 18.0),
        );
        if ui
            .put(del_rect, egui::Button::new("\u{f0e2}"))
            .on_hover_text("Discard all changes")
            .clicked()
        {
            btn_clicked = true;
            *action = Some(Action::RequestDiscard(DiscardReq {
                paths: paths.to_vec(),
                label: "all files".to_string(),
            }));
        }
    }
    let btn = if staged { "−" } else { "+" };
    let btn_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - BTN_W - 2.0, rect.center().y - 9.0),
        egui::vec2(BTN_W, 18.0),
    );
    if ui.put(btn_rect, egui::Button::new(btn)).clicked() {
        btn_clicked = true;
        *action = Some(if staged {
            Action::Unstage(paths.to_vec())
        } else {
            Action::Stage(paths.to_vec())
        });
    }
    if !btn_clicked && resp.clicked() {
        set_group_open(ui.ctx(), staged, !open);
    }
    rect
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_dir_row(
    ui: &mut egui::Ui,
    staged: bool,
    salt: &str,
    name: &str,
    open: bool,
    paths: &[String],
    depth: usize,
    is_cursor: bool,
    action: &mut Option<Action>,
) -> egui::Rect {
    let w = ui.available_width().max(40.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::click());
    let hovered = ui.rect_contains_pointer(rect);

    let text_color = ui.visuals().text_color();
    let weak = egui::Color32::from_gray(150);
    if is_cursor {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().selection.bg_fill);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let x = rect.left() + depth as f32 * INDENT;
    ui.painter().text(
        egui::pos2(x + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        if open { "▼" } else { "▶" },
        egui::FontId::proportional(10.0),
        weak,
    );
    ui.painter().text(
        egui::pos2(x + 22.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "\u{f07b}",
        egui::FontId::proportional(13.0),
        text_color,
    );
    ui.painter().text(
        egui::pos2(x + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.0),
        text_color,
    );

    let mut btn_clicked = false;
    if hovered {
        if !staged {
            let del_rect = egui::Rect::from_min_size(
                egui::pos2(rect.right() - MARKER_W - 2.0 * BTN_W, rect.center().y - 9.0),
                egui::vec2(BTN_W, 18.0),
            );
            if ui
                .put(del_rect, egui::Button::new("\u{f0e2}"))
                .on_hover_text("Discard changes in folder")
                .clicked()
            {
                btn_clicked = true;
                *action = Some(Action::RequestDiscard(DiscardReq {
                    paths: paths.to_vec(),
                    label: dir_label(salt),
                }));
            }
        }
        let btn = if staged { "−" } else { "+" };
        let btn_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - MARKER_W - BTN_W, rect.center().y - 9.0),
            egui::vec2(BTN_W, 18.0),
        );
        if ui.put(btn_rect, egui::Button::new(btn)).clicked() {
            btn_clicked = true;
            *action = Some(if staged {
                Action::Unstage(paths.to_vec())
            } else {
                Action::Stage(paths.to_vec())
            });
        }
    }
    if !btn_clicked && resp.clicked() {
        set_dir_open(ui.ctx(), salt, !open);
    }
    rect
}

pub(super) fn collect_paths(dir: &TreeDir) -> Vec<String> {
    let mut out = Vec::new();
    for sub in dir.dirs.values() {
        out.extend(collect_paths(sub));
    }
    for leaf in &dir.files {
        out.extend(file_paths(&leaf.path, leaf.old_path.as_deref()));
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_file_row(
    ui: &mut egui::Ui,
    staged: bool,
    name: &str,
    path: &str,
    old_path: Option<&str>,
    kind: StatusKind,
    depth: usize,
    is_cursor: bool,
    sel: Option<&(String, bool)>,
    action: &mut Option<Action>,
) -> egui::Rect {
    let is_sel = matches!(sel, Some((p, s)) if p == path && *s == staged);

    let w = ui.available_width().max(40.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let visuals = ui.style().interact_selectable(&resp, is_sel);

    if is_cursor {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().selection.bg_fill);
    } else if is_sel || hovered {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let x = rect.left() + depth as f32 * INDENT;
    let label = match old_path {
        Some(old) if old != path => format!("{} → {name}", basename(old)),
        _ => name.to_string(),
    };
    ui.painter().text(
        egui::pos2(x + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &label,
        egui::FontId::proportional(13.0),
        visuals.text_color(),
    );
    ui.painter().text(
        egui::pos2(rect.right() - 4.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        kind.marker(),
        egui::FontId::monospace(12.0),
        marker_color(kind),
    );

    let mut btn_clicked = false;
    if hovered {
        if !staged {
            let del_rect = egui::Rect::from_min_size(
                egui::pos2(rect.right() - MARKER_W - 2.0 * BTN_W, rect.center().y - 9.0),
                egui::vec2(BTN_W, 18.0),
            );
            if ui
                .put(del_rect, egui::Button::new("\u{f0e2}"))
                .on_hover_text("Discard changes")
                .clicked()
            {
                btn_clicked = true;
                *action = Some(Action::RequestDiscard(DiscardReq {
                    paths: file_paths(path, old_path),
                    label: path.to_string(),
                }));
            }
        }
        let btn = if staged { "−" } else { "+" };
        let btn_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - MARKER_W - BTN_W, rect.center().y - 9.0),
            egui::vec2(BTN_W, 18.0),
        );
        if ui.put(btn_rect, egui::Button::new(btn)).clicked() {
            btn_clicked = true;
            *action = Some(if staged {
                Action::Unstage(file_paths(path, old_path))
            } else {
                Action::Stage(file_paths(path, old_path))
            });
        }
    }
    if !btn_clicked {
        if resp.double_clicked() {
            *action = Some(Action::OpenEditor(path.to_string()));
        } else if resp.clicked() {
            *action = Some(Action::Select(path.to_string(), staged));
        }
    }
    rect
}

pub(super) fn marker_color(kind: StatusKind) -> egui::Color32 {
    match kind {
        StatusKind::New => egui::Color32::from_rgb(0x7e, 0xe7, 0x87),
        StatusKind::Modified => egui::Color32::from_rgb(0xe6, 0xd8, 0x6b),
        StatusKind::Deleted => egui::Color32::from_rgb(0xff, 0x7b, 0x7b),
        StatusKind::Renamed => egui::Color32::from_rgb(0x6c, 0x9c, 0xff),
        StatusKind::Conflicted => egui::Color32::from_rgb(0xff, 0xb8, 0x6c),
        StatusKind::Submodule => egui::Color32::from_rgb(0xc8, 0x9c, 0xff),
        _ => egui::Color32::from_gray(150),
    }
}

#[cfg(test)]
mod tests {
    use super::commit_guide;

    #[test]
    fn subject_counts_chars_not_bytes() {
        let g = commit_guide("日本語のコミット");
        assert_eq!(g.subject_len, 8);
        assert!(!g.line2_nonblank);
        assert!(!g.body_over_72);
    }

    #[test]
    fn flags_nonblank_line2() {
        assert!(commit_guide("subject\nbody now").line2_nonblank);
        assert!(!commit_guide("subject\n\nbody").line2_nonblank);
        assert!(!commit_guide("subject\n   \nbody").line2_nonblank);
        assert!(!commit_guide("subject only").line2_nonblank);
    }

    #[test]
    fn flags_body_over_72() {
        let long = "x".repeat(73);
        assert!(commit_guide(&format!("subject\n\n{long}")).body_over_72);
        assert!(!commit_guide(&format!("subject\n\n{}", "x".repeat(72))).body_over_72);
        assert!(!commit_guide(&format!("{long}\n\nshort body")).body_over_72);
    }
}

pub(super) fn changes_panel(app: &mut App, ui: &mut egui::Ui) {
    let rect = egui::Panel::left("changes")
        .default_size(300.0)
        .resizable(true)
        .show(ui, |ui| {
            if let Some(err) = &app.error {
                ui.colored_label(egui::Color32::RED, err);
            }
            rebase_banner(app, ui);
            let commit_clicked = commit_box(app, ui);
            remote_bar(app, ui);
            if let Some(a) = draw_stashes(app, ui) {
                match a {
                    StashAction::Pop(i) => app.stash_pop(i),
                    StashAction::Apply(i) => app.stash_apply(i),
                    StashAction::Drop(i) => app.stash_drop(i),
                }
            }

            ui.heading(format!("Changes — {}", display_name(app)));
            let mut action = None;
            egui::ScrollArea::vertical()
                .id_salt("changes")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    action = draw_changes(app, ui);
                });

            if let Some(a) = action {
                match a {
                    Action::Select(p, s) => app.select_file(p, s),
                    Action::Stage(paths) => app.stage(paths),
                    Action::Unstage(paths) => app.unstage(paths),
                    Action::OpenEditor(p) => app.open_in_editor(&p),
                    Action::RequestDiscard(req) => {
                        if app.config.confirm_discard {
                            app.confirm_discard = Some(req);
                        } else {
                            app.discard_paths(&req.paths);
                        }
                    }
                }
            }
            if commit_clicked {
                app.do_commit();
            }
            focus_on_click(app, ui, Pane::Changes);
        })
        .response
        .rect;
    pane_border(ui, rect, app.focus == Pane::Changes);
}

fn commit_box(app: &mut App, ui: &mut egui::Ui) -> bool {
    let mut commit_clicked = false;
    let mut stash_clicked = false;
    let can_amend = app.can_amend();
    let mut amend_toggle: Option<bool> = None;
    egui::Panel::top("commit_box")
        .resizable(false)
        .show(ui, |ui| {
            ui.add_space(4.0);
            let hint = if app.amend_mode {
                "Amend message"
            } else {
                "Commit message"
            };
            ui.add(
                egui::TextEdit::multiline(&mut app.commit_msg)
                    .hint_text(hint)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                let label = if app.amend_mode {
                    format!("Amend ({} staged)", app.staged.len())
                } else {
                    format!("Commit ({} staged)", app.staged.len())
                };
                commit_clicked = ui.button(label).clicked();
                stash_clicked = ui
                    .button("\u{f187}  Stash")
                    .on_hover_text("Stash all changes (incl. untracked)")
                    .clicked();
                let mut amend = app.amend_mode;
                let cb = ui
                    .add_enabled(can_amend, egui::Checkbox::new(&mut amend, "Amend"))
                    .on_hover_text("Replace the last commit with the staged changes");
                if cb.changed() {
                    amend_toggle = Some(amend);
                }
                if app.config.commit_message_guide {
                    commit_guide_row(app, ui);
                }
            });
            ui.add_space(4.0);
        });
    if let Some(on) = amend_toggle {
        app.set_amend_mode(on);
    }
    if stash_clicked {
        app.stash_push();
    }
    commit_clicked
}
