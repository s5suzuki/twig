use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use git2::Oid;

use crate::repo::{CommitFile, Graph, RefKind, RefLabel, ResetMode, Segment, StatusKind};

const HEAD_COLOR: Color32 = Color32::from_rgb(0xff, 0xd1, 0x6b);

const ROW_H: f32 = 22.0;
const FILE_H: f32 = 18.0;
const COL_W: f32 = 14.0;
const NODE_R: f32 = 4.0;
const PAD_LEFT: f32 = 8.0;
const TEXT_GAP: f32 = 10.0;
const FILE_PAD: f32 = 4.0;

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

pub enum GraphAction {
    Commit(Oid),
    File(String),
    RebaseOnto(Oid),
    CherryPick(Oid),
    Revert(Oid),
    Switch(String),
    CheckoutRemote(String),
    CheckoutCommit(Oid),
    CreateBranch(Oid),
    RenameBranch(String),
    DeleteBranch(String),
    CreateTag(Oid),
    DeleteTag(String),
    Reset(Oid, ResetMode),
    StashPop(usize),
    StashApply(usize),
    StashDrop(usize),
}

fn parse_stash_index(name: &str) -> Option<usize> {
    name.strip_prefix("stash@{")?
        .strip_suffix('}')?
        .parse()
        .ok()
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    graph: &Graph,
    ui: &mut egui::Ui,
    selected: Option<Oid>,
    files: &[CommitFile],
    sel_file: Option<&str>,
    show_author: bool,
    show_date: bool,
) -> Option<GraphAction> {
    if graph.rows.is_empty() {
        ui.weak("(no commits)");
        return None;
    }

    let expanded_row = selected.and_then(|s| graph.rows.iter().position(|r| r.id == s));
    let file_block_h = if expanded_row.is_some() && !files.is_empty() {
        files.len() as f32 * FILE_H + FILE_PAD
    } else {
        0.0
    };

    let gutter = PAD_LEFT + (graph.max_col as f32 + 1.0) * COL_W;
    let total_h = graph.rows.len() as f32 * ROW_H + file_block_h;
    let width = ui.available_width().max(gutter + 200.0);

    let (rect, resp) = ui.allocate_exact_size(vec2(width, total_h), Sense::click());
    let painter = ui.painter_at(rect);

    let x = |col: usize| rect.left() + PAD_LEFT + col as f32 * COL_W + COL_W / 2.0;
    let id_color = Color32::from_gray(130);
    let text_color = ui.visuals().text_color();
    let sel_bg = ui.visuals().selection.bg_fill;
    let hover_bg = ui.visuals().widgets.hovered.weak_bg_fill;
    let hover_pos = resp.hover_pos();

    let mut commit_hits: Vec<(f32, f32, Oid)> = Vec::new();
    let mut file_hits: Vec<(f32, f32, String)> = Vec::new();

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
            for (k, f) in files.iter().enumerate() {
                let fy = y_top + ROW_H + k as f32 * FILE_H;
                let fr = Rect::from_min_max(pos2(rect.left(), fy), pos2(rect.right(), fy + FILE_H));
                if sel_file == Some(f.path.as_str()) {
                    painter.rect_filled(fr, 0.0, sel_bg);
                } else if hover_pos.is_some_and(|p| fr.contains(p)) {
                    painter.rect_filled(fr, 0.0, hover_bg);
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

    let menu_id = egui::Id::new("graph_menu_target");
    if resp.secondary_clicked()
        && let Some(p) = resp.interact_pointer_pos() {
            let hit = commit_hits.iter().find(|(a, b, _)| p.y >= *a && p.y < *b);
            ui.ctx()
                .data_mut(|d| d.insert_temp(menu_id, hit.map(|(_, _, oid)| *oid)));
        }

    let mut menu_action = None;
    resp.context_menu(|ui| {
        let target: Option<Oid> = ui.ctx().data(|d| d.get_temp(menu_id)).flatten();
        let Some(oid) = target else {
            ui.close();
            return;
        };
        let row = graph.rows.iter().find(|r| r.id == oid);

        if let Some(stash) = row.and_then(|r| r.refs.iter().find(|x| x.kind == RefKind::Stash)) {
            if let Some(idx) = parse_stash_index(&stash.name) {
                ui.label(&stash.name);
                ui.separator();
                if ui.button("\u{f0ab}  Pop (apply + drop)").clicked() {
                    menu_action = Some(GraphAction::StashPop(idx));
                    ui.close();
                }
                if ui.button("\u{f0c5}  Apply (keep)").clicked() {
                    menu_action = Some(GraphAction::StashApply(idx));
                    ui.close();
                }
                if ui.button("\u{f1f8}  Drop").clicked() {
                    menu_action = Some(GraphAction::StashDrop(idx));
                    ui.close();
                }
            }
            return;
        }

        let short = oid.to_string();
        ui.label(format!("commit {}", &short[..7.min(short.len())]));
        ui.separator();

        if let Some(row) = row {
            for rf in &row.refs {
                match rf.kind {
                    RefKind::LocalBranch => {
                        if !rf.is_head
                            && ui
                                .button(format!("\u{e725}  Switch to {}", rf.name))
                                .clicked()
                        {
                            menu_action = Some(GraphAction::Switch(rf.name.clone()));
                            ui.close();
                        }
                        if ui
                            .button(format!("\u{f044}  Rename {}\u{2026}", rf.name))
                            .clicked()
                        {
                            menu_action = Some(GraphAction::RenameBranch(rf.name.clone()));
                            ui.close();
                        }
                        if !rf.is_head
                            && ui.button(format!("\u{f1f8}  Delete {}", rf.name)).clicked()
                        {
                            menu_action = Some(GraphAction::DeleteBranch(rf.name.clone()));
                            ui.close();
                        }
                    }
                    RefKind::RemoteBranch
                        if ui
                            .button(format!("\u{e725}  Checkout {} as local branch", rf.name))
                            .clicked()
                        => {
                            menu_action = Some(GraphAction::CheckoutRemote(rf.name.clone()));
                            ui.close();
                        }
                    RefKind::Tag
                        if ui
                            .button(format!("\u{f1f8}  Delete tag {}", rf.name))
                            .clicked()
                        => {
                            menu_action = Some(GraphAction::DeleteTag(rf.name.clone()));
                            ui.close();
                        }
                    _ => {}
                }
            }
        }
        if ui.button("\u{f067}  Create branch here\u{2026}").clicked() {
            menu_action = Some(GraphAction::CreateBranch(oid));
            ui.close();
        }
        if ui.button("\u{f02b}  Create tag here\u{2026}").clicked() {
            menu_action = Some(GraphAction::CreateTag(oid));
            ui.close();
        }
        if ui.button("\u{f06a}  Checkout commit (detached)").clicked() {
            menu_action = Some(GraphAction::CheckoutCommit(oid));
            ui.close();
        }
        ui.separator();
        if ui
            .button("\u{e729}  Cherry-pick onto current branch")
            .clicked()
        {
            menu_action = Some(GraphAction::CherryPick(oid));
            ui.close();
        }
        if ui.button("\u{f0e2}  Revert this commit").clicked() {
            menu_action = Some(GraphAction::Revert(oid));
            ui.close();
        }
        if ui
            .button("\u{e728}  Rebase current branch onto this")
            .clicked()
        {
            menu_action = Some(GraphAction::RebaseOnto(oid));
            ui.close();
        }
        ui.menu_button("\u{f0e2}  Reset current branch to here", |ui| {
            for mode in [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard] {
                if ui.button(mode.label()).clicked() {
                    menu_action = Some(GraphAction::Reset(oid, mode));
                    ui.close();
                }
            }
        });
    });
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
