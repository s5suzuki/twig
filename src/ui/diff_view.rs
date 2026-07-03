use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    Align, Color32, FontId, Galley, Label, Layout, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextFormat, pos2, text::LayoutJob, vec2,
};

use crate::highlight::{DiffHighlighter, Span};
use crate::repo::{DiffRow, FileDiff, LineKind};

#[derive(Default)]
pub struct DiffGalleyCache {
    sig: Option<(u64, bool)>,
    left: HashMap<usize, Arc<Galley>>,
    right: HashMap<usize, Arc<Galley>>,
}

impl DiffGalleyCache {
    fn sync(&mut self, ver: u64, dark: bool) {
        let sig = (ver, dark);
        if self.sig != Some(sig) {
            self.sig = Some(sig);
            self.left.clear();
            self.right.clear();
        }
    }
}

const ADD_BG_DARK: Color32 = Color32::from_rgb(0x1c, 0x3a, 0x24);
const DEL_BG_DARK: Color32 = Color32::from_rgb(0x40, 0x20, 0x24);
const ADD_BG_LIGHT: Color32 = Color32::from_rgb(0xcc, 0xef, 0xd0);
const DEL_BG_LIGHT: Color32 = Color32::from_rgb(0xf5, 0xd2, 0xd8);

const ADD_EMPH_DARK: Color32 = Color32::from_rgb(0x2e, 0x6a, 0x3e);
const DEL_EMPH_DARK: Color32 = Color32::from_rgb(0x7a, 0x28, 0x35);
const ADD_EMPH_LIGHT: Color32 = Color32::from_rgb(0x86, 0xd8, 0x92);
const DEL_EMPH_LIGHT: Color32 = Color32::from_rgb(0xf0, 0x9d, 0xaa);
const HUNK_FG: Color32 = Color32::from_rgb(0x6c, 0x9c, 0xff);
const NO_FG: Color32 = Color32::from_gray(110);

const RULER_ADD: Color32 = Color32::from_rgb(0x4b, 0xb5, 0x4e);
const RULER_DEL: Color32 = Color32::from_rgb(0xd9, 0x4f, 0x4f);
const RULER_MOD: Color32 = Color32::from_rgb(0x4a, 0x90, 0xe2);

const RULER_W: f32 = 14.0;
const COL_GAP: f32 = 6.0;
const OVERSCAN: f32 = 300.0;

fn add_bg(dark: bool) -> Color32 {
    if dark { ADD_BG_DARK } else { ADD_BG_LIGHT }
}
fn del_bg(dark: bool) -> Color32 {
    if dark { DEL_BG_DARK } else { DEL_BG_LIGHT }
}
fn add_emph(dark: bool) -> Color32 {
    if dark { ADD_EMPH_DARK } else { ADD_EMPH_LIGHT }
}
fn del_emph(dark: bool) -> Color32 {
    if dark { DEL_EMPH_DARK } else { DEL_EMPH_LIGHT }
}

pub struct DiffNav {
    pub cursor: usize,
    pub sel: Option<(usize, usize)>,
    pub scroll_to_cursor: bool,
    pub center: bool,
}

#[derive(Default)]
pub struct FindRender {
    pub rows: HashMap<usize, Vec<(usize, usize, bool)>>,
}

#[derive(Default)]
pub struct DiffResponse {
    pub hunk_toggle: Option<usize>,
    pub drag_select: Option<(usize, usize)>,
    pub visible: Option<(usize, usize)>,
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    diff: &FileDiff,
    ui: &mut egui::Ui,
    hunk_ctl: Option<bool>,
    nav: Option<&DiffNav>,
    find: Option<&FindRender>,
    hl: &mut DiffHighlighter,
    cache: &mut DiffGalleyCache,
    ver: u64,
) -> DiffResponse {
    let mut resp = DiffResponse::default();
    if let Some(note) = &diff.note {
        ui.weak(note);
        return resp;
    }

    let full = ui.available_size();
    let target_id = ui.id().with("diff_scroll_target");
    let pending: Option<f32> = ui.memory(|m| m.data.get_temp(target_id));

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let content_w = (full.x - RULER_W).max(0.0);

        let out = ui
            .allocate_ui_with_layout(
                vec2(content_w, full.y),
                Layout::top_down(Align::LEFT),
                |ui| {
                    let mut sa = ScrollArea::vertical()
                        .id_salt("diff")
                        .auto_shrink([false, false]);
                    if let Some(t) = pending {
                        sa = sa.vertical_scroll_offset(t);
                    }
                    sa.show(ui, |ui| {
                        render_rows(diff, ui, hunk_ctl, nav, find, hl, cache, ver, &mut resp)
                    })
                },
            )
            .inner;

        if pending.is_some() {
            ui.memory_mut(|m| m.data.remove::<f32>(target_id));
        }

        let (rect, ruler_resp) =
            ui.allocate_exact_size(vec2(RULER_W, full.y), Sense::click_and_drag());
        draw_ruler(ui, rect, &ruler_resp, &diff.rows, &out, target_id);
    });

    resp
}

#[allow(clippy::too_many_arguments)]
fn render_rows(
    diff: &FileDiff,
    ui: &mut egui::Ui,
    hunk_ctl: Option<bool>,
    nav: Option<&DiffNav>,
    find: Option<&FindRender>,
    hl: &mut DiffHighlighter,
    cache: &mut DiffGalleyCache,
    ver: u64,
    resp: &mut DiffResponse,
) {
    ui.spacing_mut().item_spacing.y = 1.0;
    let content_w = ui.available_width();
    let sel_fill = ui.visuals().selection.bg_fill;
    let sel_overlay = Color32::from_rgba_unmultiplied(sel_fill.r(), sel_fill.g(), sel_fill.b(), 60);
    let cursor_bar = ui.visuals().selection.stroke.color;
    let dark_mode = ui.visuals().dark_mode;
    let hl_color = if dark_mode {
        Color32::from_rgb(0x5c, 0x51, 0x1e)
    } else {
        Color32::from_rgb(0xff, 0xe4, 0x82)
    };
    let cur_color = if dark_mode {
        Color32::from_rgb(0xc0, 0x76, 0x1a)
    } else {
        Color32::from_rgb(0xff, 0xb3, 0x4d)
    };
    let empty_hl: Vec<(usize, usize, bool)> = Vec::new();
    let clip = ui.clip_rect();
    let mut line_rects: Vec<(usize, Rect)> = Vec::new();
    let mut visible: Option<(usize, usize)> = None;

    let char_w = ui
        .ctx()
        .fonts_mut(|f| f.glyph_width(&FontId::monospace(11.0), '0'));
    let max_no = diff
        .rows
        .iter()
        .filter_map(|r| match r {
            DiffRow::Line { old_no, new_no, .. } => old_no
                .or(*new_no)
                .map(|_| old_no.unwrap_or(0).max(new_no.unwrap_or(0))),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    let digits = max_no.to_string().len().max(2);
    let lineno_w = digits as f32 * char_w + 8.0;
    let text_w = ((content_w - 2.0 * lineno_w - 3.0 * COL_GAP - 2.0) / 2.0).max(40.0);
    let default_color = ui.visuals().text_color();
    let min_row_h = ui
        .ctx()
        .fonts_mut(|f| f.row_height(&FontId::monospace(12.0)));
    cache.sync(ver, dark_mode);

    for (i, row) in diff.rows.iter().enumerate() {
        match row {
            DiffRow::FileHeader(path) => {
                ui.add_space(4.0);
                ui.add(
                    Label::new(
                        RichText::new(path)
                            .strong()
                            .font(FontId::proportional(13.0)),
                    )
                    .wrap(),
                );
                ui.separator();
            }
            DiffRow::Hunk { index, header } => {
                ui.horizontal(|ui| {
                    if let Some(staged) = hunk_ctl {
                        let (glyph, hint) = if staged {
                            ("−", "Unstage hunk")
                        } else {
                            ("+", "Stage hunk")
                        };
                        if ui.small_button(glyph).on_hover_text(hint).clicked() {
                            resp.hunk_toggle = Some(*index);
                        }
                    }
                    ui.add(
                        Label::new(
                            RichText::new(header)
                                .font(FontId::monospace(12.0))
                                .color(HUNK_FG),
                        )
                        .wrap(),
                    );
                });
            }
            DiffRow::Line {
                old_no,
                new_no,
                left,
                right,
                kind,
                left_emph,
                right_emph,
            } => {
                let y = ui.cursor().top();
                let force = nav.is_some_and(|n| n.scroll_to_cursor && n.cursor == i);
                let onscreen = force
                    || (y + min_row_h >= clip.top() - OVERSCAN && y <= clip.bottom() + OVERSCAN);
                if !onscreen {
                    let (rrect, _) =
                        ui.allocate_exact_size(vec2(content_w, min_row_h), Sense::hover());
                    if nav.is_some() {
                        line_rects.push((i, rrect));
                    }
                    continue;
                }
                let dark = ui.visuals().dark_mode;
                let (left_bg, right_bg) = match kind {
                    LineKind::Context => (None, None),
                    LineKind::Added => (None, Some(add_bg(dark))),
                    LineKind::Removed => (Some(del_bg(dark)), None),
                    LineKind::Changed => (Some(del_bg(dark)), Some(add_bg(dark))),
                };
                let find_hl = find.and_then(|f| f.rows.get(&i)).unwrap_or(&empty_hl);
                hl.ensure_upto(&diff.rows, i);
                let left_syn = hl.left(i);
                let right_syn = hl.right(i);

                let colors = CellColors {
                    default_color,
                    hl_color,
                    cur_color,
                };
                let left_galley = cell_galley(
                    ui,
                    &mut cache.left,
                    i,
                    left.as_deref(),
                    text_w,
                    left_syn,
                    &[],
                    left_emph,
                    del_emph(dark),
                    colors,
                );
                let right_galley = cell_galley(
                    ui,
                    &mut cache.right,
                    i,
                    right.as_deref(),
                    text_w,
                    right_syn,
                    find_hl,
                    right_emph,
                    add_emph(dark),
                    colors,
                );

                let row_resp = ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = COL_GAP;
                    lineno_cell(ui, *old_no, lineno_w);
                    paint_cell(ui, &left_galley, text_w, left_bg, min_row_h, default_color);
                    lineno_cell(ui, *new_no, lineno_w);
                    paint_cell(
                        ui,
                        &right_galley,
                        text_w,
                        right_bg,
                        min_row_h,
                        default_color,
                    );
                });
                let rrect = row_resp.response.rect;
                if let Some(nav) = nav {
                    let selected = nav.sel.is_some_and(|(lo, hi)| i >= lo && i <= hi);
                    if selected {
                        ui.painter().rect_filled(rrect, 0.0, sel_overlay);
                    }
                    if nav.cursor == i {
                        ui.painter().rect_filled(
                            Rect::from_min_size(rrect.min, vec2(2.0, rrect.height())),
                            0.0,
                            cursor_bar,
                        );
                        if nav.scroll_to_cursor {
                            let align = if nav.center { Some(Align::Center) } else { None };
                            ui.scroll_to_rect_animation(
                                rrect,
                                align,
                                egui::style::ScrollAnimation::none(),
                            );
                        }
                    }
                    if rrect.bottom() >= clip.top() && rrect.top() <= clip.bottom() {
                        visible = Some(match visible {
                            Some((a, _)) => (a, i),
                            None => (i, i),
                        });
                    }
                    line_rects.push((i, rrect));
                }
            }
        }
    }

    if nav.is_some() {
        resp.visible = visible;
        handle_drag(ui, &line_rects, resp);
    }
}

fn handle_drag(ui: &egui::Ui, line_rects: &[(usize, Rect)], resp: &mut DiffResponse) {
    if line_rects.is_empty() {
        return;
    }
    let id = ui.id().with("diff_drag_anchor");

    let clip = ui.clip_rect();
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let released = ui.input(|i| i.pointer.primary_released());
    let pos = ui.input(|i| i.pointer.interact_pos());

    let left = line_rects
        .iter()
        .map(|(_, r)| r.left())
        .fold(f32::MAX, f32::min);
    let right = line_rects
        .iter()
        .map(|(_, r)| r.right())
        .fold(0.0_f32, f32::max);
    let row_exact = |y: f32| {
        line_rects
            .iter()
            .find(|(_, r)| y >= r.top() && y < r.bottom())
    };

    let row_clamped = |y: f32| -> usize {
        let mut chosen = line_rects[0].0;
        for (idx, r) in line_rects {
            if r.top() <= y {
                chosen = *idx;
            } else {
                break;
            }
        }
        chosen
    };

    if pressed
        && let Some(p) = pos
        && clip.contains(p)
        && p.x >= left
        && p.x <= right
        && let Some((i, _)) = row_exact(p.y)
    {
        ui.memory_mut(|m| m.data.insert_temp(id, *i));
    }
    if down
        && let Some(anchor) = ui.memory(|m| m.data.get_temp::<usize>(id))
        && let Some(p) = pos
    {
        let y = p.y.clamp(clip.top(), clip.bottom() - 1.0);
        resp.drag_select = Some((anchor, row_clamped(y)));
    }
    if released {
        ui.memory_mut(|m| m.data.remove::<usize>(id));
    }
}

fn lineno_cell(ui: &mut egui::Ui, no: Option<u32>, w: f32) {
    let txt = no.map(|n| n.to_string()).unwrap_or_default();
    ui.scope(|ui| {
        ui.set_min_width(w);
        ui.set_max_width(w);
        ui.with_layout(Layout::top_down(Align::Max), |ui| {
            ui.add(Label::new(
                RichText::new(txt)
                    .font(FontId::monospace(11.0))
                    .color(NO_FG),
            ));
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn cell_galley(
    ui: &egui::Ui,
    cache: &mut HashMap<usize, Arc<Galley>>,
    i: usize,
    text: Option<&str>,
    w: f32,
    syn: &[Span],
    find: &[(usize, usize, bool)],
    emph: &[std::ops::Range<usize>],
    emph_color: Color32,
    colors: CellColors,
) -> Arc<Galley> {
    if syn.is_empty() {
        return build_galley(ui, text, w, syn, find, emph, emph_color, colors);
    }
    if find.is_empty() {
        return cache
            .entry(i)
            .or_insert_with(|| build_galley(ui, text, w, syn, &[], emph, emph_color, colors))
            .clone();
    }
    build_galley(ui, text, w, syn, find, emph, emph_color, colors)
}

#[derive(Clone, Copy)]
struct CellColors {
    default_color: Color32,
    hl_color: Color32,
    cur_color: Color32,
}

#[allow(clippy::too_many_arguments)]
fn build_galley(
    ui: &egui::Ui,
    text: Option<&str>,
    w: f32,
    syn: &[Span],
    find: &[(usize, usize, bool)],
    emph: &[std::ops::Range<usize>],
    emph_color: Color32,
    colors: CellColors,
) -> Arc<Galley> {
    let _ = w;
    let text = text.unwrap_or("");
    let font = FontId::monospace(12.0);
    let color = colors.default_color;
    if syn.is_empty() && find.is_empty() && emph.is_empty() {
        return ui
            .painter()
            .layout(text.to_owned(), font, color, f32::INFINITY);
    }

    let len = text.len();
    let mut bounds = vec![0usize, len];
    for &(s, e, _) in syn {
        bounds.push(s.min(len));
        bounds.push(e.min(len));
    }
    for &(s, e, _) in find {
        bounds.push(s.min(len));
        bounds.push(e.min(len));
    }
    for r in emph {
        bounds.push(r.start.min(len));
        bounds.push(r.end.min(len));
    }
    bounds.retain(|&b| text.is_char_boundary(b));
    bounds.sort_unstable();
    bounds.dedup();

    let fg_at = |pos: usize| -> Color32 {
        syn.iter()
            .find(|&&(s, e, _)| pos >= s && pos < e)
            .map(|&(_, _, c)| c)
            .unwrap_or(color)
    };
    let bg_at = |pos: usize| -> Option<Color32> {
        if let Some(c) =
            find.iter()
                .find(|&&(s, e, _)| pos >= s && pos < e)
                .map(|&(_, _, is_cur)| {
                    if is_cur {
                        colors.cur_color
                    } else {
                        colors.hl_color
                    }
                })
        {
            return Some(c);
        }
        if emph.iter().any(|r| pos >= r.start && pos < r.end) {
            return Some(emph_color);
        }
        None
    };

    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for pair in bounds.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a >= b {
            continue;
        }
        let mut f = TextFormat::simple(font.clone(), fg_at(a));
        if let Some(bg) = bg_at(a) {
            f.background = bg;
        }
        job.append(&text[a..b], 0.0, f);
    }
    ui.painter().layout_job(job)
}

fn paint_cell(
    ui: &mut egui::Ui,
    galley: &Arc<Galley>,
    w: f32,
    bg: Option<Color32>,
    min_row_h: f32,
    color: Color32,
) {
    let h = galley.size().y.max(min_row_h);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if let Some(c) = bg {
        ui.painter().rect_filled(rect, 0.0, c);
    }
    ui.painter()
        .with_clip_rect(rect)
        .galley(rect.min, galley.clone(), color);
}

fn draw_ruler(
    ui: &mut egui::Ui,
    rect: Rect,
    resp: &egui::Response,
    rows: &[DiffRow],
    out: &egui::scroll_area::ScrollAreaOutput<()>,
    target_id: egui::Id,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

    let n = rows.len().max(1) as f32;
    let tick_h = (rect.height() / n).max(2.0);
    for (i, row) in rows.iter().enumerate() {
        if let DiffRow::Line { kind, .. } = row {
            let col = match kind {
                LineKind::Context => continue,
                LineKind::Added => RULER_ADD,
                LineKind::Removed => RULER_DEL,
                LineKind::Changed => RULER_MOD,
            };
            let y = rect.top() + (i as f32 / n) * rect.height();
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.left() + 2.0, y), vec2(rect.width() - 4.0, tick_h)),
                0.0,
                col,
            );
        }
    }

    let content_h = out.content_size.y;
    let view_h = out.inner_rect.height();
    if content_h > view_h + 1.0 {
        let off = out.state.offset.y;
        let y0 = rect.top() + (off / content_h) * rect.height();
        let y1 = rect.top() + ((off + view_h) / content_h) * rect.height();
        let vr = Rect::from_min_max(pos2(rect.left(), y0), pos2(rect.right(), y1));
        painter.rect_filled(vr, 2.0, Color32::from_white_alpha(18));
        painter.rect_stroke(
            vr,
            2.0,
            Stroke::new(1.0, Color32::from_white_alpha(60)),
            StrokeKind::Inside,
        );
    }

    if (resp.clicked() || resp.dragged())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let frac = ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        let target = (frac * content_h - view_h / 2.0).clamp(0.0, (content_h - view_h).max(0.0));
        ui.memory_mut(|m| m.data.insert_temp(target_id, target));
        ui.ctx().request_repaint();
    }
}
