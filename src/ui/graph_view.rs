use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2};
use git2::Oid;

use crate::app::GraphMenu;
use crate::repo::{CommitFile, Graph, RefKind, RefLabel, Segment, StatusKind};

const HEAD_COLOR: Color32 = Color32::from_rgb(0xff, 0xd1, 0x6b);

const ROW_H: f32 = 22.0;
const FILE_H: f32 = 18.0;
const COL_W: f32 = 14.0;
const NODE_R: f32 = 4.0;
const PAD_LEFT: f32 = 8.0;
const TEXT_GAP: f32 = 10.0;
const FILE_PAD: f32 = 4.0;
const MSG_PAD: f32 = 6.0;

const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x4f, 0xc1, 0xff),
    Color32::from_rgb(0xc5, 0x95, 0xff),
    Color32::from_rgb(0x7e, 0xe7, 0x87),
    Color32::from_rgb(0xff, 0xb8, 0x6c),
    Color32::from_rgb(0xff, 0x7b, 0x9c),
    Color32::from_rgb(0xe6, 0xd8, 0x6b),
    Color32::from_rgb(0x6c, 0x9c, 0xff),
    Color32::from_rgb(0x9c, 0x9c, 0x9c),
];

fn lane_color(i: usize) -> Color32 {
    PALETTE[i % PALETTE.len()]
}

fn kind_color(kind: StatusKind) -> Color32 {
    match kind {
        StatusKind::New => Color32::from_rgb(0x7e, 0xe7, 0x87),
        StatusKind::Modified => Color32::from_rgb(0xe6, 0xd8, 0x6b),
        StatusKind::Deleted => Color32::from_rgb(0xff, 0x7b, 0x7b),
        StatusKind::Renamed => Color32::from_rgb(0x6c, 0x9c, 0xff),
        _ => Color32::from_gray(150),
    }
}

#[derive(Clone)]
pub enum GraphAction {
    Commit(Oid),
    File(String),
    RebaseOnto(Oid),
    InteractiveRebase(Oid),
    CherryPick(Oid),
    Revert(Oid),
    Switch(String),
    CheckoutRemote(String),
    DeleteRemoteBranch(String),
    CheckoutCommit(Oid),
    CreateBranch(Oid),
    RenameBranch(String),
    DeleteBranch(String),
    CreateTag(Oid),
    DeleteTag(String),
    OpenReset(Oid),
    StashPop(usize),
    StashApply(usize),
    StashDrop(usize),
    Push,
    ForcePush,
    Fetch,
}

fn parse_stash_index(name: &str) -> Option<usize> {
    name.strip_prefix("stash@{")?
        .strip_suffix('}')?
        .parse()
        .ok()
}

pub struct GraphCursor {
    pub commit_row: Option<usize>,
    pub file: Option<usize>,
    pub scroll: bool,
    pub open_menu: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    graph: &Graph,
    ui: &mut egui::Ui,
    selected: Option<Oid>,
    files: &[CommitFile],
    message: &str,
    sel_file: Option<&str>,
    show_author: bool,
    show_date: bool,
    cursor: Option<&GraphCursor>,
    menu: &mut Option<GraphMenu>,
) -> Option<GraphAction> {
    if graph.rows.is_empty() {
        ui.weak("(no commits)");
        return None;
    }

    let expanded_row = selected.and_then(|s| graph.rows.iter().position(|r| r.id == s));

    let gutter = PAD_LEFT + (graph.max_col as f32 + 1.0) * COL_W;
    let width = ui.available_width().max(gutter + 200.0);

    let msg_font = FontId::monospace(12.0);
    let msg_wrap = (width - gutter - MSG_PAD * 2.0).max(80.0);
    let msg_text = message.trim_end();
    let msg_galley = if expanded_row.is_some() && !msg_text.is_empty() {
        Some(ui.ctx().fonts_mut(|f| {
            f.layout(
                msg_text.to_string(),
                msg_font,
                ui.visuals().text_color(),
                msg_wrap,
            )
        }))
    } else {
        None
    };
    let msg_h = msg_galley
        .as_ref()
        .map(|g| g.size().y + MSG_PAD * 2.0)
        .unwrap_or(0.0);
    let files_h = if expanded_row.is_some() && !files.is_empty() {
        files.len() as f32 * FILE_H + FILE_PAD
    } else {
        0.0
    };
    let file_block_h = msg_h + files_h;

    let total_h = graph.rows.len() as f32 * ROW_H + file_block_h;

    let (rect, resp) = ui.allocate_exact_size(vec2(width, total_h), Sense::click());
    let painter = ui.painter_at(rect);

    let x = |col: usize| rect.left() + PAD_LEFT + col as f32 * COL_W + COL_W / 2.0;
    let id_color = Color32::from_gray(130);
    let text_color = ui.visuals().text_color();
    let sel_bg = ui.visuals().selection.bg_fill;
    let hover_bg = ui.visuals().widgets.hovered.weak_bg_fill;
    let detail_bg = ui.visuals().faint_bg_color;
    let cursor_stroke = Stroke::new(1.5, ui.visuals().selection.stroke.color);
    let hover_pos = resp.hover_pos();
    let cursor_commit = cursor.and_then(|c| c.commit_row);
    let cursor_file = cursor.and_then(|c| c.file);

    let mut commit_hits: Vec<(f32, f32, Oid)> = Vec::new();
    let mut file_hits: Vec<(f32, f32, String)> = Vec::new();
    let mut cursor_rect: Option<Rect> = None;

    let mut y = rect.top();
    for (i, row) in graph.rows.iter().enumerate() {
        let is_expanded = expanded_row == Some(i);
        let extra = if is_expanded { file_block_h } else { 0.0 };
        let y_top = y;
        let row_h = ROW_H + extra;
        let y_bot = y_top + row_h;
        let y_mid = y_top + ROW_H / 2.0;
        let node_x = x(row.node_col);

        let commit_band =
            Rect::from_min_max(pos2(rect.left(), y_top), pos2(rect.right(), y_top + ROW_H));
        if selected == Some(row.id) {
            painter.rect_filled(commit_band, 0.0, sel_bg);
        } else if hover_pos.is_some_and(|p| commit_band.contains(p)) {
            painter.rect_filled(commit_band, 0.0, hover_bg);
        }
        if cursor_commit == Some(i) {
            painter.rect_stroke(commit_band, 0.0, cursor_stroke, StrokeKind::Inside);
            cursor_rect = Some(commit_band);
        }

        for seg in &row.segments {
            match *seg {
                Segment::Through { col, color } => {
                    line(&painter, pos2(x(col), y_top), pos2(x(col), y_bot), color);
                }
                Segment::TopToNode { col, color } => {
                    line(&painter, pos2(x(col), y_top), pos2(node_x, y_mid), color);
                }
                Segment::NodeToBottom { col, color } => {
                    line(&painter, pos2(node_x, y_mid), pos2(x(col), y_bot), color);
                }
            }
        }

        painter.circle_filled(pos2(node_x, y_mid), NODE_R, lane_color(row.node_color));
        if row.is_head {
            painter.circle_stroke(
                pos2(node_x, y_mid),
                NODE_R + 2.5,
                Stroke::new(1.6, HEAD_COLOR),
            );
        }

        let id_rect = painter.text(
            pos2(rect.left() + gutter, y_mid),
            Align2::LEFT_CENTER,
            &row.short_id,
            FontId::monospace(12.0),
            id_color,
        );

        let mut bx = id_rect.right() + TEXT_GAP;
        for label in &row.refs {
            bx += draw_ref_badge(&painter, bx, y_mid, label);
        }

        let summary_rect = painter.text(
            pos2(bx, y_mid),
            Align2::LEFT_CENTER,
            &row.summary,
            FontId::proportional(13.0),
            text_color,
        );

        let mut meta_x = summary_rect.right() + TEXT_GAP * 1.5;
        if show_author && !row.author.is_empty() {
            let r = painter.text(
                pos2(meta_x, y_mid),
                Align2::LEFT_CENTER,
                &row.author,
                FontId::proportional(12.0),
                id_color,
            );
            meta_x = r.right() + TEXT_GAP;
        }
        if show_date && !row.date.is_empty() {
            painter.text(
                pos2(meta_x, y_mid),
                Align2::LEFT_CENTER,
                &row.date,
                FontId::monospace(11.0),
                id_color,
            );
        }

        commit_hits.push((y_top, y_top + ROW_H, row.id));

        if is_expanded {
            let detail_top = y_top + ROW_H;
            let detail_rect = Rect::from_min_max(
                pos2(rect.left(), detail_top),
                pos2(rect.right(), detail_top + file_block_h),
            );
            painter.rect_filled(detail_rect, 0.0, detail_bg);

            if let Some(galley) = &msg_galley {
                let msg_rect = Rect::from_min_size(
                    pos2(rect.left() + gutter, detail_top + MSG_PAD),
                    vec2(msg_wrap, galley.size().y),
                );
                ui.scope_builder(UiBuilder::new().max_rect(msg_rect), |ui| {
                    ui.add(egui::Label::new(galley.clone()).selectable(true));
                });
            }

            let files_top = detail_top + msg_h;
            for (k, f) in files.iter().enumerate() {
                let fy = files_top + k as f32 * FILE_H;
                let fr = Rect::from_min_max(pos2(rect.left(), fy), pos2(rect.right(), fy + FILE_H));
                if sel_file == Some(f.path.as_str()) {
                    painter.rect_filled(fr, 0.0, sel_bg);
                } else if hover_pos.is_some_and(|p| fr.contains(p)) {
                    painter.rect_filled(fr, 0.0, hover_bg);
                }
                if cursor_file == Some(k) {
                    painter.rect_stroke(fr, 0.0, cursor_stroke, StrokeKind::Inside);
                    cursor_rect = Some(fr);
                }
                let fy_mid = fy + FILE_H / 2.0;
                let mx = rect.left() + gutter + 6.0;
                painter.text(
                    pos2(mx, fy_mid),
                    Align2::LEFT_CENTER,
                    "\u{2514}",
                    FontId::proportional(11.0),
                    Color32::from_gray(110),
                );
                painter.text(
                    pos2(mx + 14.0, fy_mid),
                    Align2::LEFT_CENTER,
                    f.kind.marker().to_string(),
                    FontId::monospace(12.0),
                    kind_color(f.kind),
                );
                painter.text(
                    pos2(mx + 30.0, fy_mid),
                    Align2::LEFT_CENTER,
                    &f.path,
                    FontId::proportional(12.0),
                    text_color,
                );
                file_hits.push((fy, fy + FILE_H, f.path.clone()));
            }
        }

        y = y_bot;
    }

    if cursor.is_some_and(|c| c.scroll)
        && let Some(r) = cursor_rect
    {
        ui.scroll_to_rect(r, None);
    }

    let kb_open = cursor.is_some_and(|c| c.open_menu);
    if kb_open {
        let kb_oid = cursor_commit
            .map(|i| graph.rows[i].id)
            .or(if cursor_file.is_some() { selected } else { None });
        if let (Some(oid), Some(r)) = (kb_oid, cursor_rect) {
            *menu = Some(GraphMenu {
                oid,
                pos: pos2(r.left() + gutter, r.bottom()),
                cursor: 0,
            });
        }
    } else if resp.secondary_clicked()
        && let Some(p) = resp.interact_pointer_pos()
        && let Some((_, _, oid)) = commit_hits.iter().find(|(a, b, _)| p.y >= *a && p.y < *b)
    {
        *menu = Some(GraphMenu {
            oid: *oid,
            pos: p,
            cursor: 0,
        });
    }

    let mut menu_action = None;
    if let Some((oid, pos, mut sel)) = menu.as_ref().map(|gm| (gm.oid, gm.pos, gm.cursor)) {
        let row = graph.rows.iter().find(|r| r.id == oid);
        let (header, entries) = build_menu_entries(oid, row);
        let popup_id = ui.make_persistent_id("graph_context_menu");
        let mut open = true;
        let mut close = false;
        let inner = egui::Popup::menu(&resp)
            .id(popup_id)
            .at_position(pos)
            .layout(egui::Layout::top_down(egui::Align::Min))
            .open_bool(&mut open)
            .show(|ui| draw_menu(ui, &header, &entries, &mut sel, &mut close));
        menu_action = inner.and_then(|i| i.inner);
        if !open || close || menu_action.is_some() {
            *menu = None;
        } else if let Some(gm) = menu.as_mut() {
            gm.cursor = sel;
        }
    }
    if menu_action.is_some() {
        return menu_action;
    }

    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos() {
            if let Some((_, _, path)) = file_hits.iter().find(|(a, b, _)| p.y >= *a && p.y < *b) {
                return Some(GraphAction::File(path.clone()));
            }
            if let Some((_, _, oid)) = commit_hits.iter().find(|(a, b, _)| p.y >= *a && p.y < *b) {
                return Some(GraphAction::Commit(*oid));
            }
        }
    None
}

fn build_menu_entries(
    oid: Oid,
    row: Option<&crate::repo::GraphRow>,
) -> (String, Vec<(String, GraphAction)>) {
    if let Some(stash) = row.and_then(|r| r.refs.iter().find(|x| x.kind == RefKind::Stash))
        && let Some(idx) = parse_stash_index(&stash.name)
    {
        let entries = vec![
            ("\u{f0ab}  Pop (apply + drop)".to_string(), GraphAction::StashPop(idx)),
            ("\u{f0c5}  Apply (keep)".to_string(), GraphAction::StashApply(idx)),
            ("\u{f1f8}  Drop".to_string(), GraphAction::StashDrop(idx)),
        ];
        return (stash.name.clone(), entries);
    }

    let mut entries: Vec<(String, GraphAction)> = Vec::new();
    if let Some(row) = row {
        for rf in &row.refs {
            match rf.kind {
                RefKind::LocalBranch => {
                    if !rf.is_head {
                        entries.push((
                            format!("\u{e725}  Switch to {}", rf.name),
                            GraphAction::Switch(rf.name.clone()),
                        ));
                    }
                    entries.push((
                        format!("\u{f044}  Rename {}\u{2026}", rf.name),
                        GraphAction::RenameBranch(rf.name.clone()),
                    ));
                    if !rf.is_head {
                        entries.push((
                            format!("\u{f1f8}  Delete {}", rf.name),
                            GraphAction::DeleteBranch(rf.name.clone()),
                        ));
                    }
                }
                RefKind::RemoteBranch => {
                    entries.push((
                        format!("\u{e725}  Checkout {} as local branch", rf.name),
                        GraphAction::CheckoutRemote(rf.name.clone()),
                    ));
                    entries.push((
                        format!("\u{f1f8}  Delete {} on remote", rf.name),
                        GraphAction::DeleteRemoteBranch(rf.name.clone()),
                    ));
                }
                RefKind::Tag => {
                    entries.push((
                        format!("\u{f1f8}  Delete tag {}", rf.name),
                        GraphAction::DeleteTag(rf.name.clone()),
                    ));
                }
                _ => {}
            }
        }
    }
    entries.push((
        "\u{f067}  Create branch here\u{2026}".to_string(),
        GraphAction::CreateBranch(oid),
    ));
    entries.push((
        "\u{f02b}  Create tag here\u{2026}".to_string(),
        GraphAction::CreateTag(oid),
    ));
    entries.push((
        "\u{f06a}  Checkout commit (detached)".to_string(),
        GraphAction::CheckoutCommit(oid),
    ));
    entries.push((
        "\u{e729}  Cherry-pick onto current branch".to_string(),
        GraphAction::CherryPick(oid),
    ));
    entries.push((
        "\u{f0e2}  Revert this commit".to_string(),
        GraphAction::Revert(oid),
    ));
    entries.push((
        "\u{e728}  Rebase current branch onto this".to_string(),
        GraphAction::RebaseOnto(oid),
    ));
    entries.push((
        "\u{e728}  Interactive rebase from here\u{2026}".to_string(),
        GraphAction::InteractiveRebase(oid),
    ));
    entries.push((
        "\u{f0e2}  Reset current branch to here\u{2026}".to_string(),
        GraphAction::OpenReset(oid),
    ));
    entries.push(("\u{f021}  Fetch".to_string(), GraphAction::Fetch));
    entries.push(("\u{f0aa}  Push".to_string(), GraphAction::Push));
    entries.push(("\u{f0aa}  Force push".to_string(), GraphAction::ForcePush));

    let short = oid.to_string();
    (format!("commit {}", &short[..7.min(short.len())]), entries)
}

const MENU_ROW_H: f32 = 20.0;

fn draw_menu(
    ui: &mut egui::Ui,
    header: &str,
    entries: &[(String, GraphAction)],
    sel: &mut usize,
    close: &mut bool,
) -> Option<GraphAction> {
    let n = entries.len();
    if n == 0 {
        *close = true;
        return None;
    }
    *sel = (*sel).min(n - 1);

    let (mut down, mut up, mut activate) = (false, false, false);
    ui.input_mut(|i| {
        down = i.consume_key(egui::Modifiers::NONE, egui::Key::J)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
        up = i.consume_key(egui::Modifiers::NONE, egui::Key::K)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
        activate = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::L)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
        if i.consume_key(egui::Modifiers::NONE, egui::Key::H)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
        {
            *close = true;
        }
    });
    if down {
        *sel = (*sel + 1) % n;
    }
    if up {
        *sel = (*sel + n - 1) % n;
    }

    let font = FontId::proportional(13.0);
    ui.add(egui::Label::new(egui::RichText::new(header).weak().monospace()).selectable(false));
    ui.separator();

    let mut width = 60.0f32;
    for (label, _) in entries {
        let w = ui.ctx().fonts_mut(|f| {
            f.layout_no_wrap(label.clone(), font.clone(), Color32::WHITE)
                .size()
                .x
        });
        width = width.max(w);
    }
    width += 16.0;

    let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let sel_bg = ui.visuals().selection.bg_fill;
    let text_color = ui.visuals().text_color();

    let mut result = None;
    for (i, (label, action)) in entries.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(width, MENU_ROW_H), Sense::click());
        if resp.hovered() && pointer_moved {
            *sel = i;
        }
        if *sel == i {
            ui.painter().rect_filled(rect, 3.0, sel_bg);
        }
        ui.painter().text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            font.clone(),
            text_color,
        );
        if resp.clicked() {
            result = Some(action.clone());
        }
    }

    if result.is_none() && activate {
        result = Some(entries[*sel].1.clone());
    }
    result
}

fn line(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, color: usize) {
    painter.line_segment([a, b], Stroke::new(1.8, lane_color(color)));
}

fn draw_ref_badge(painter: &egui::Painter, x: f32, y_mid: f32, label: &RefLabel) -> f32 {
    let (bg, fg, icon) = badge_style(label);
    let font = FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(format!("{icon} {}", label.name), font, fg);
    let (tw, th) = (galley.size().x, galley.size().y);
    const PAD_X: f32 = 5.0;
    const H: f32 = 16.0;
    let rect = Rect::from_min_size(pos2(x, y_mid - H / 2.0), vec2(tw + PAD_X * 2.0, H));
    painter.rect_filled(rect, 4.0, bg);
    if label.is_head {
        painter.rect_stroke(rect, 4.0, Stroke::new(1.2, HEAD_COLOR), StrokeKind::Inside);
    }
    painter.galley(pos2(rect.left() + PAD_X, y_mid - th / 2.0), galley, fg);
    rect.width() + 6.0
}

fn badge_style(label: &RefLabel) -> (Color32, Color32, &'static str) {
    const BRANCH: &str = "\u{e725}";
    const TAG: &str = "\u{f02b}";
    const DETACHED: &str = "\u{f06a}";
    match label.kind {
        RefKind::LocalBranch if label.is_head => {
            (Color32::from_rgb(0x33, 0x6b, 0x3d), Color32::WHITE, BRANCH)
        }
        RefKind::LocalBranch => (
            Color32::from_rgb(0x2d, 0x4f, 0x6b),
            Color32::from_rgb(0xcf, 0xe6, 0xff),
            BRANCH,
        ),
        RefKind::RemoteBranch => (
            Color32::from_rgb(0x3a, 0x32, 0x52),
            Color32::from_rgb(0xd8, 0xc8, 0xff),
            BRANCH,
        ),
        RefKind::Tag => (
            Color32::from_rgb(0x5a, 0x4a, 0x1f),
            Color32::from_rgb(0xff, 0xe6, 0x9c),
            TAG,
        ),
        RefKind::DetachedHead => (
            Color32::from_rgb(0x6b, 0x2e, 0x2e),
            Color32::WHITE,
            DETACHED,
        ),
        RefKind::Stash => (
            Color32::from_rgb(0x2f, 0x4a, 0x4a),
            Color32::from_rgb(0xa9, 0xe6, 0xe6),
            "\u{f187}",
        ),
    }
}
