use super::*;

pub(super) fn find_bar(app: &mut App, ui: &mut egui::Ui) {
    let staged = app.selected_file.as_ref().map(|(_, s)| *s).unwrap_or(false);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let mut go_next = false;
        let mut go_prev = false;
        ui.horizontal(|ui| {
            ui.label("\u{f002}");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut app.find.query)
                    .hint_text("Find")
                    .desired_width(220.0),
            );
            if app.find.focus_request {
                resp.request_focus();
                app.find.focus_request = false;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if ui.input(|i| i.modifiers.shift) {
                    go_prev = true;
                } else {
                    go_next = true;
                }
                app.find.focus_request = true;
            }
            if ui
                .selectable_label(app.find.case_sensitive, "Aa")
                .on_hover_text("Match case")
                .clicked()
            {
                app.find.case_sensitive = !app.find.case_sensitive;
            }
            if ui
                .selectable_label(app.find.regex, ".*")
                .on_hover_text("Regular expression")
                .clicked()
            {
                app.find.regex = !app.find.regex;
            }
            let status = if let Some(e) = &app.find.error {
                e.clone()
            } else if app.find.query.is_empty() {
                String::new()
            } else if app.find.matches.is_empty() {
                "No results".to_string()
            } else {
                format!("{}/{}", app.find.current + 1, app.find.matches.len())
            };
            ui.add(egui::Label::new(status).truncate());
            if ui
                .button("\u{f062}")
                .on_hover_text("Previous (Shift+Enter)")
                .clicked()
            {
                go_prev = true;
            }
            if ui
                .button("\u{f063}")
                .on_hover_text("Next (Enter)")
                .clicked()
            {
                go_next = true;
            }
            if ui.button("\u{f00d}").on_hover_text("Close (Esc)").clicked() {
                app.close_find();
            }
        });
        if !staged {
            ui.horizontal(|ui| {
                ui.label("\u{f3a5}");
                ui.add(
                    egui::TextEdit::singleline(&mut app.find.replace)
                        .hint_text("Replace")
                        .desired_width(220.0),
                );
                if ui.button("Replace").clicked() {
                    app.find_replace_current();
                }
                if ui
                    .button(format!("Replace all ({})", app.find.matches.len()))
                    .clicked()
                {
                    app.find_replace_all();
                }
            });
        }
        if go_prev {
            app.find_prev();
        }
        if go_next {
            app.find_next();
        }
    });
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.close_find();
    }
}

pub(super) fn remote_bar(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!app.remote_busy, |ui| {
            if ui
                .button("\u{f021}  Fetch")
                .on_hover_text("Fetch from remote")
                .clicked()
            {
                app.fetch(&ctx);
            }
            if ui
                .button("\u{f0ab}  Pull")
                .on_hover_text("Fetch + merge into the current branch")
                .clicked()
            {
                app.pull(&ctx);
            }
            let push = ui
                .button("\u{f0aa}  Push")
                .on_hover_text("Push the current branch (right-click to force)");
            if push.clicked() {
                app.push(&ctx, false);
            }
            push.context_menu(|ui| {
                if ui.button("\u{f0aa}  Force push (current branch)").clicked() {
                    app.request_force_push();
                    ui.close();
                }
            });
        });
        if app.remote_busy {
            ui.spinner();
            let verb = match app.remote_kind {
                crate::app::RemoteKind::Fetch => "Fetching",
                crate::app::RemoteKind::Pull => "Pulling",
                crate::app::RemoteKind::Push => "Pushing",
                crate::app::RemoteKind::DeleteRemote => "Deleting",
                crate::app::RemoteKind::SubmoduleInit => "Initializing submodule",
                crate::app::RemoteKind::SubmoduleUpdate => "Updating submodule",
            };
            let text = match app.remote_progress {
                Some((r, t)) if t > 0 => format!("{verb} {r}/{t}"),
                _ => format!("{verb}\u{2026}"),
            };
            ui.weak(text);
        }
    });
}

pub(super) fn rebase_banner(app: &mut App, ui: &mut egui::Ui) {
    let Some(status) = &app.seq else {
        return;
    };
    if matches!(status.kind, crate::app::SeqKind::RebaseInteractive) {
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(0x3a, 0x2a, 0x1a))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(0xff, 0xb8, 0x6c), "\u{e728}");
                    ui.strong("Interactive rebase in progress");
                });
                ui.label("Drive it from the terminal below (continue / abort / edit there).");
            });
        return;
    }

    let conflicts = status.conflicts.clone();
    let title = format!("{} in progress", status.kind.label());
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(0x3a, 0x2a, 0x1a))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(0xff, 0xb8, 0x6c), "\u{e728}");
                ui.strong(title);
            });
            if conflicts.is_empty() {
                ui.label("Resolve and stage changes, then Continue.");
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(0xff, 0x7b, 0x7b),
                    format!("Conflicts ({}):", conflicts.len()),
                );
                for f in &conflicts {
                    ui.weak(format!("  {f}"));
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("\u{f04b}  Continue").clicked() {
                    app.seq_continue();
                }
                if ui
                    .add(
                        egui::Button::new("\u{f04d}  Abort")
                            .fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)),
                    )
                    .clicked()
                {
                    app.seq_abort();
                }
            });
        });
}
