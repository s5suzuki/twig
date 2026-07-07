use super::*;

pub(super) fn draw_settings(app: &mut App, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    use twit_core::config::{Accent, Theme};
    let mut open = app.settings_open;
    let mut apply = false;
    let mut fonts = false;
    let mut reload = false;
    let mut save = false;
    egui::Window::new("\u{f013}  Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(300.0);

            ui.label("Font size");
            let r = ui.add(
                egui::Slider::new(&mut app.config.font_size, 9.0..=24.0)
                    .step_by(1.0)
                    .suffix(" pt"),
            );
            if r.drag_stopped() || r.lost_focus() {
                apply = true;
                save = true;
            }

            let monos = crate::fonts::available_mono();
            if monos.len() > 1 {
                ui.add_space(4.0);
                let cur = monos
                    .iter()
                    .find(|m| m.key == app.config.mono_font)
                    .map(|m| m.label)
                    .unwrap_or("(default)");
                egui::ComboBox::from_label("Code font")
                    .selected_text(cur)
                    .show_ui(ui, |ui| {
                        for m in &monos {
                            if ui
                                .selectable_value(
                                    &mut app.config.mono_font,
                                    m.key.to_string(),
                                    m.label,
                                )
                                .changed()
                            {
                                fonts = true;
                                save = true;
                            }
                        }
                    });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.label("Color theme");
            for theme in Theme::ALL {
                if ui
                    .radio_value(&mut app.config.theme, theme, theme.label())
                    .changed()
                {
                    apply = true;
                    save = true;
                }
            }
            if app.config.theme == Theme::CatppuccinMocha {
                ui.add_space(4.0);
                egui::ComboBox::from_label("Accent")
                    .selected_text(app.config.accent.label())
                    .show_ui(ui, |ui| {
                        for a in Accent::ALL {
                            if ui
                                .selectable_value(&mut app.config.accent, a, a.label())
                                .changed()
                            {
                                apply = true;
                                save = true;
                            }
                        }
                    });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.label("Graph");
            ui.horizontal(|ui| {
                ui.label("Commits shown");
                let r = ui.add(
                    egui::DragValue::new(&mut app.config.graph_commit_limit)
                        .range(50..=5000)
                        .speed(5),
                );
                if r.drag_stopped() || r.lost_focus() {
                    reload = true;
                    save = true;
                }
            });
            if ui
                .checkbox(&mut app.config.graph_show_author, "Show author")
                .changed()
            {
                save = true;
            }
            if ui
                .checkbox(&mut app.config.graph_show_date, "Show date")
                .changed()
            {
                save = true;
            }
            ui.horizontal(|ui| {
                ui.label("Commit files");
                if ui
                    .selectable_value(&mut app.config.graph_files_tree, true, "\u{f07b}  Tree")
                    .changed()
                {
                    save = true;
                }
                if ui
                    .selectable_value(&mut app.config.graph_files_tree, false, "\u{f03a}  List")
                    .changed()
                {
                    save = true;
                }
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.label("Behavior");
            if ui
                .checkbox(
                    &mut app.config.confirm_discard,
                    "Confirm before discarding changes",
                )
                .changed()
            {
                save = true;
            }
            if ui
                .checkbox(
                    &mut app.config.commit_message_guide,
                    "Show commit message length guide",
                )
                .changed()
            {
                save = true;
            }
            if ui
                .checkbox(
                    &mut app.config.show_files,
                    "Show repository files in sidebar",
                )
                .changed()
            {
                save = true;
            }
            if ui
                .checkbox(&mut app.config.show_title_bar, "Show window title bar")
                .changed()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(
                    app.config.show_title_bar,
                ));
                save = true;
            }
        });
    app.settings_open = open;

    if apply {
        app.apply_config(ctx);
    }
    if fonts {
        crate::fonts::install(ctx, &app.config.mono_font);
    }
    if reload {
        app.reload();
    }
    if save {
        app.config.save();
    }
}
