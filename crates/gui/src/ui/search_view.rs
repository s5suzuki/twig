use egui::{Color32, FontId, TextFormat, text::LayoutJob};

use crate::app::App;

pub enum SearchAction {
    OpenEditor(String),
}

enum Intent {
    ToggleFile(usize),
    ToggleLine(String, u32),
    Open(String),
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
    egui::ScrollArea::vertical()
        .id_salt("search_results")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let results = &app.search.results;
            let selected = &app.search.selected;
            for (fi, f) in results.iter().enumerate() {
                let all = f
                    .lines
                    .iter()
                    .all(|l| selected.contains(&(f.path.clone(), l.line_no)));
                ui.horizontal(|ui| {
                    let mut checked = all;
                    if ui.checkbox(&mut checked, "").changed() {
                        intents.push(Intent::ToggleFile(fi));
                    }
                    if ui
                        .add(
                            egui::Label::new(egui::RichText::new(&f.path).strong())
                                .sense(egui::Sense::click()),
                        )
                        .clicked()
                    {
                        intents.push(Intent::Open(f.path.clone()));
                    }
                    ui.weak(format!("({})", f.lines.len()));
                });
                for l in &f.lines {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.add_space(18.0);
                        let mut checked = selected.contains(&(f.path.clone(), l.line_no));
                        if ui.checkbox(&mut checked, "").changed() {
                            intents.push(Intent::ToggleLine(f.path.clone(), l.line_no));
                        }
                        ui.add(egui::Label::new(
                            egui::RichText::new(format!("{:>5}", l.line_no))
                                .font(FontId::monospace(11.0))
                                .color(Color32::from_gray(120)),
                        ));
                        let job = highlight_job(&l.text, &l.ranges, ui.visuals().text_color());
                        if ui
                            .add(egui::Label::new(job).truncate().sense(egui::Sense::click()))
                            .clicked()
                        {
                            intents.push(Intent::Open(f.path.clone()));
                        }
                    });
                }
            }
        });

    let mut action = None;
    for it in intents {
        match it {
            Intent::ToggleFile(i) => app.search_toggle_file(i),
            Intent::ToggleLine(p, ln) => app.search_toggle_line(&p, ln),
            Intent::Open(p) => action = Some(SearchAction::OpenEditor(p)),
        }
    }
    action
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
