use std::collections::HashSet;

use egui::{Color32, FontId, TextFormat, text::LayoutJob};
use twit_core::search::FileHit;

use crate::app::App;

pub enum SearchAction {
    OpenEditor { path: String, line: u32 },
}

enum Intent {
    ToggleFile(usize),
    ToggleLine(String, u32),
    ToggleDir(String),
    ToggleFoldDir(String),
    ToggleFoldFile(String),
    Open { path: String, line: u32 },
}

#[derive(Default)]
struct DirNode {
    dirs: Vec<(String, DirNode)>,
    files: Vec<usize>,
}

fn build_tree(results: &[FileHit]) -> DirNode {
    let mut root = DirNode::default();
    for (i, f) in results.iter().enumerate() {
        let comps: Vec<&str> = f.path.split('/').collect();
        let mut node = &mut root;
        for c in &comps[..comps.len().saturating_sub(1)] {
            let idx = match node.dirs.iter().position(|(n, _)| n == c) {
                Some(p) => p,
                None => {
                    node.dirs.push(((*c).to_string(), DirNode::default()));
                    node.dirs.len() - 1
                }
            };
            node = &mut node.dirs[idx].1;
        }
        node.files.push(i);
    }
    root
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) -> Option<SearchAction> {
    let mut run = false;
    ui.horizontal(|ui| {
        ui.label("\u{f002}");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.search.query)
                .hint_text("Search in repository")
                .desired_width(280.0),
        );
        if app.search.focus_request {
            resp.request_focus();
            app.search.focus_request = false;
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            run = true;
        }
        if ui
            .selectable_label(app.search.case_sensitive, "Aa")
            .on_hover_text("Match case")
            .clicked()
        {
            app.search.case_sensitive = !app.search.case_sensitive;
        }
        if ui
            .selectable_label(app.search.regex, ".*")
            .on_hover_text("Regular expression")
            .clicked()
        {
            app.search.regex = !app.search.regex;
        }
        if ui.button("Search").clicked() {
            run = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("\u{f3a5}");
        ui.add(
            egui::TextEdit::singleline(&mut app.search.replace)
                .hint_text("Replace with")
                .desired_width(280.0),
        );
        let sel = app.search.selected_count();
        ui.add_enabled_ui(sel > 0 && !app.search.query.is_empty(), |ui| {
            if ui.button(format!("Replace\u{2026} ({sel})")).clicked() {
                app.search_confirm = true;
            }
        });
        if ui.button("All").clicked() {
            app.search_select_all(true);
        }
        if ui.button("None").clicked() {
            app.search_select_all(false);
        }
    });

    ui.horizontal(|ui| {
        ui.label("\u{f0b0}").on_hover_text("Files to include / exclude (glob)");
        let inc = ui.add(
            egui::TextEdit::singleline(&mut app.search.include)
                .hint_text("files to include")
                .desired_width(180.0),
        );
        let exc = ui.add(
            egui::TextEdit::singleline(&mut app.search.exclude)
                .hint_text("files to exclude")
                .desired_width(180.0),
        );
        let entered = (inc.lost_focus() || exc.lost_focus())
            && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if entered && !app.search.query.is_empty() {
            run = true;
        }
    });

    if run {
        app.search_run();
    }
    if let Some(e) = &app.search.error {
        ui.colored_label(Color32::RED, e);
    }
    ui.separator();

    if app.search.searched && app.search.results.is_empty() && app.search.error.is_none() {
        ui.weak("No matches");
        return None;
    }
    if !app.search.results.is_empty() {
        let total: usize = app.search.results.iter().map(|f| f.lines.len()).sum();
        ui.weak(format!(
            "{} match(es) in {} file(s)",
            total,
            app.search.results.len()
        ));
    }

    let mut intents: Vec<Intent> = Vec::new();
    let tree = build_tree(&app.search.results);
    egui::ScrollArea::vertical()
        .id_salt("search_results")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut ctx = RenderCtx {
                app,
                intents: &mut intents,
            };
            draw_dir(ui, &tree, "", 0, &mut ctx);
        });

    let mut action = None;
    for it in intents {
        match it {
            Intent::ToggleFile(i) => app.search_toggle_file(i),
            Intent::ToggleLine(p, ln) => app.search_toggle_line(&p, ln),
            Intent::ToggleDir(p) => app.search_toggle_dir(&p),
            Intent::ToggleFoldDir(p) => {
                if !app.search.folded_dirs.remove(&p) {
                    app.search.folded_dirs.insert(p);
                }
            }
            Intent::ToggleFoldFile(p) => {
                if !app.search.folded_files.remove(&p) {
                    app.search.folded_files.insert(p);
                }
            }
            Intent::Open { path, line } => action = Some(SearchAction::OpenEditor { path, line }),
        }
    }
    action
}

struct RenderCtx<'a> {
    app: &'a App,
    intents: &'a mut Vec<Intent>,
}

const INDENT: f32 = 14.0;

fn draw_dir(ui: &mut egui::Ui, node: &DirNode, prefix: &str, depth: usize, ctx: &mut RenderCtx) {
    for (name, child) in &node.dirs {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let folded = ctx.app.search.folded_dirs.contains(&full);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(depth as f32 * INDENT);
            let mut checked = ctx.app.search_dir_all_selected(&full);
            if ui.checkbox(&mut checked, "").changed() {
                ctx.intents.push(Intent::ToggleDir(full.clone()));
            }
            let arrow = if folded { "\u{25b6}" } else { "\u{25bc}" };
            if ui
                .add(
                    egui::Label::new(format!("{arrow} \u{f07b}  {name}"))
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                ctx.intents.push(Intent::ToggleFoldDir(full.clone()));
            }
        });
        if !folded {
            draw_dir(ui, child, &full, depth + 1, ctx);
        }
    }
    for &fi in &node.files {
        draw_file(ui, fi, depth, ctx);
    }
}

fn draw_file(ui: &mut egui::Ui, fi: usize, depth: usize, ctx: &mut RenderCtx) {
    let Some(f) = ctx.app.search.results.get(fi) else {
        return;
    };
    let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
    let first_line = f.lines.first().map(|l| l.line_no).unwrap_or(1);
    let folded = ctx.app.search.folded_files.contains(&f.path);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(depth as f32 * INDENT);
        let mut checked = ctx.app.search_file_all_selected(f);
        if ui.checkbox(&mut checked, "").changed() {
            ctx.intents.push(Intent::ToggleFile(fi));
        }
        let arrow = if folded { "\u{25b6}" } else { "\u{25bc}" };
        if ui
            .add(egui::Label::new(arrow).sense(egui::Sense::click()))
            .clicked()
        {
            ctx.intents.push(Intent::ToggleFoldFile(f.path.clone()));
        }
        if ui
            .add(
                egui::Label::new(egui::RichText::new(format!("\u{f0f6}  {name}")).strong())
                    .sense(egui::Sense::click()),
            )
            .clicked()
        {
            ctx.intents.push(Intent::Open {
                path: f.path.clone(),
                line: first_line,
            });
        }
        ui.weak(format!("({})", f.lines.len()));
    });
    if folded {
        return;
    }
    let text_color = ui.visuals().text_color();
    let selected: &HashSet<(String, u32)> = &ctx.app.search.selected;
    for l in &f.lines {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add_space((depth + 1) as f32 * INDENT + 4.0);
            let mut checked = selected.contains(&(f.path.clone(), l.line_no));
            if ui.checkbox(&mut checked, "").changed() {
                ctx.intents.push(Intent::ToggleLine(f.path.clone(), l.line_no));
            }
            ui.add(egui::Label::new(
                egui::RichText::new(format!("{:>5}", l.line_no))
                    .font(FontId::monospace(11.0))
                    .color(Color32::from_gray(120)),
            ));
            let job = highlight_job(&l.text, &l.ranges, text_color);
            if ui
                .add(egui::Label::new(job).truncate().sense(egui::Sense::click()))
                .clicked()
            {
                ctx.intents.push(Intent::Open {
                    path: f.path.clone(),
                    line: l.line_no,
                });
            }
        });
    }
}

fn highlight_job(text: &str, ranges: &[(usize, usize)], color: Color32) -> LayoutJob {
    let font = FontId::monospace(12.0);
    let hl = Color32::from_rgb(0x5c, 0x51, 0x1e);
    let mut job = LayoutJob::default();
    let mut pos = 0usize;
    for &(s, e) in ranges {
        let s = s.min(text.len());
        let e = e.min(text.len());
        if s < pos || e < s {
            continue;
        }
        if pos < s {
            job.append(&text[pos..s], 0.0, TextFormat::simple(font.clone(), color));
        }
        let mut f = TextFormat::simple(font.clone(), color);
        f.background = hl;
        job.append(&text[s..e], 0.0, f);
        pos = e;
    }
    if pos < text.len() {
        job.append(&text[pos..], 0.0, TextFormat::simple(font.clone(), color));
    }
    job
}
