use super::*;

pub(super) fn help_key(app: &mut App, ui: &mut egui::Ui) {
    if app.terminal_focused() || ui.ctx().memory(|m| m.focused().is_some()) {
        return;
    }
    if !app.help_open && app.any_modal_open() {
        return;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Questionmark)) {
        app.help_open = !app.help_open;
    } else if app.help_open
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        app.help_open = false;
    }
}

pub(super) fn draw_help(app: &mut App, ctx: &egui::Context) {
    if !app.help_open {
        return;
    }
    let focused = app.help_context();
    let resp = egui::Modal::new(egui::Id::new("keybindings_help")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.horizontal(|ui| {
            ui.heading("\u{f11c}  Keybindings");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("? or Esc to close");
            });
        });
        ui.add_space(4.0);
        ui.label("Bindings active in the focused pane, plus global ones.");
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .max_height(ctx.content_rect().height() * 0.7)
            .show(ui, |ui| {
                if let Some(section) = focused {
                    help_section(ui, &app.keymap, section);
                    ui.add_space(10.0);
                }
                help_section(ui, &app.keymap, crate::keys::Context::Global);
            });
    });
    if resp.should_close() {
        app.help_open = false;
    }
}

pub(super) fn help_section(
    ui: &mut egui::Ui,
    keymap: &crate::keys::Keymap,
    ctx: crate::keys::Context,
) {
    ui.label(egui::RichText::new(ctx.title()).strong());
    ui.add_space(2.0);
    egui::Grid::new(("help_grid", ctx.title()))
        .num_columns(2)
        .spacing([16.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for e in keymap.help_for(ctx) {
                ui.add(
                    egui::Label::new(egui::RichText::new(&e.keys).monospace()).selectable(false),
                );
                ui.add(egui::Label::new(e.desc).selectable(false));
                ui.end_row();
            }
        });
}
