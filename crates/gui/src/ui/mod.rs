pub mod diff_view;
mod graph_view;
mod search_view;
mod sidebar;

use std::collections::BTreeMap;

use crate::app::{App, Dir, DiscardReq, Pane, Tab};
use twit_core::config::{Accent, Theme};
use twit_core::repo::{StatusEntry, StatusKind};

mod bars;
mod changes;
mod help;
mod input;
mod modals;
mod settings;
mod tabs;
use bars::*;
use changes::*;
use help::*;
use input::*;
use modals::*;
use settings::*;
use tabs::*;

const BTN_W: f32 = 22.0;
const MARKER_W: f32 = 16.0;
const INDENT: f32 = 14.0;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    app.track_nav();
    help_key(app, ui);
    if !app.help_open && !app.any_modal_open() {
        handle_global_keys(app, ui);
        diff_keys(app, ui);
    }
    if app.active_tab != Tab::Graph {
        app.graph_menu = None;
    }

    app.ensure_watcher(ui.ctx());
    app.update_visibility(ui.ctx());
    app.poll_remote();
    if app.take_external_change() {
        app.refresh_from_disk();
    }
    app.poll_diff_recheck(ui.ctx());

    repos_panel(app, ui);
    changes_panel(app, ui);
    terminal_panel(app, ui);
    central_panel(app, ui);

    confirm_discard_modal(app, ui);
    confirm_discard_range_modal(app, ui);
    ref_prompt_modal(app, ui);
    delete_ref_modal(app, ui);
    reset_modal(app, ui);
    confirm_op_modal(app, ui);
    force_push_modal(app, ui);
    amend_confirm_modal(app, ui);
    search_confirm_modal(app, ui);

    draw_settings(app, ui.ctx());
    draw_help(app, ui.ctx());
}

fn repos_panel(app: &mut App, ui: &mut egui::Ui) {
    let rect = egui::Panel::left("repos")
        .default_size(200.0)
        .resizable(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(egui::RichText::new("Repositories").heading()).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .selectable_label(app.config.show_files, "\u{f0f6}")
                        .on_hover_text("Show repository files")
                        .clicked()
                    {
                        app.config.show_files = !app.config.show_files;
                        app.config.save();
                    }
                });
            });
            ui.separator();
            sidebar::draw_tree(app, ui);
            focus_on_click(app, ui, Pane::Sidebar);
        })
        .response
        .rect;
    pane_border(ui, rect, app.focus == Pane::Sidebar);
}

fn terminal_panel(app: &mut App, ui: &mut egui::Ui) {
    if !app.shell_open {
        return;
    }
    let rect = egui::Panel::bottom("terminal")
        .default_size(220.0)
        .resizable(true)
        .show(ui, |ui| {
            app.ensure_shell(ui.ctx());
            app.flush_pending_shell_cmd();
            let active = app.focus == Pane::Terminal;
            if let Some(t) = &mut app.shell
                && t.ui(ui, active)
            {
                app.focus = Pane::Terminal;
            }
        })
        .response
        .rect;
    pane_border(ui, rect, app.focus == Pane::Terminal);
}

fn focus_on_click(app: &mut App, ui: &egui::Ui, pane: Pane) {
    if ui.rect_contains_pointer(ui.max_rect()) && ui.input(|i| i.pointer.any_pressed()) {
        app.focus = pane;
    }
}

fn pane_border(ui: &egui::Ui, rect: egui::Rect, active: bool) {
    if !active {
        return;
    }
    let stroke = egui::Stroke::new(2.0, ui.visuals().selection.bg_fill);
    ui.painter()
        .rect_stroke(rect.shrink(1.0), 0.0, stroke, egui::StrokeKind::Inside);
}

fn display_name(app: &App) -> String {
    app.selected
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| app.selected.display().to_string())
}

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}
