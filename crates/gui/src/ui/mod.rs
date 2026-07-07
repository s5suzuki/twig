pub mod diff_view;
mod graph_view;
mod search_view;
mod sidebar;

use std::collections::BTreeMap;

use crate::app::{App, DiscardReq, Dir, Pane, Tab};
use twit_core::config::{Accent, Theme};
use twit_core::repo::{StatusEntry, StatusKind};

const BTN_W: f32 = 22.0;
const MARKER_W: f32 = 16.0;
const INDENT: f32 = 14.0;

enum Action {
    Select(String, bool),
    Stage(Vec<String>),
    Unstage(Vec<String>),
    OpenEditor(String),
    RequestDiscard(DiscardReq),
}

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

    let repos_rect = egui::Panel::left("repos")
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
    pane_border(ui, repos_rect, app.focus == Pane::Sidebar);

    let changes_rect = egui::Panel::left("changes")
        .default_size(300.0)
        .resizable(true)
        .show(ui, |ui| {
            if let Some(err) = &app.error {
                ui.colored_label(egui::Color32::RED, err);
            }
            rebase_banner(app, ui);

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
    pane_border(ui, changes_rect, app.focus == Pane::Changes);

    if app.shell_open {
        let term_rect = egui::Panel::bottom("terminal")
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
        pane_border(ui, term_rect, app.focus == Pane::Terminal);
    }

    let central_rect = egui::CentralPanel::default()
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.active_tab, Tab::Graph, "\u{e725}  Graph");
                let diff_label = if let Some(p) = &app.selected_commit_file {
                    format!("\u{f417}  {}", basename(p))
                } else if let Some((_, label)) = &app.selected_commit {
                    format!("\u{f417}  {label}")
                } else if let Some((p, _)) = &app.selected_file {
                    format!("\u{f0f6}  {}", basename(p))
                } else {
                    "\u{f0f6}  Diff".to_string()
                };
                ui.selectable_value(&mut app.active_tab, Tab::Diff, diff_label);
                ui.selectable_value(&mut app.active_tab, Tab::Search, "\u{f002}  Search");
                ui.selectable_value(&mut app.active_tab, Tab::Editor, "\u{f040}  Editor");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("\u{f013}").on_hover_text("Settings").clicked() {
                        app.settings_open = true;
                    }
                    if ui
                        .selectable_label(app.shell_open, "\u{f489}  Terminal")
                        .clicked()
                    {
                        app.toggle_shell();
                    }
                });
            });
            ui.separator();

            match app.active_tab {
                Tab::Graph => {
                    let ctx = ui.ctx().clone();
                    let graph_focused = app.focus == Pane::RightTab;
                    let open_menu = if graph_focused {
                        graph_keys(app, ui)
                    } else {
                        false
                    };
                    let cursor = if graph_focused {
                        app.clamp_graph_cursor();
                        let items = app.graph_items();
                        let (commit_row, file, folder) = match items.get(app.graph_cursor) {
                            Some(crate::app::GraphItem::Commit(r)) => (Some(*r), None, None),
                            Some(crate::app::GraphItem::File(k)) => (None, Some(*k), None),
                            Some(crate::app::GraphItem::Folder(p)) => (None, None, Some(p.clone())),
                            _ => (None, None, None),
                        };
                        Some(graph_view::GraphCursor {
                            commit_row,
                            file,
                            folder,
                            scroll: app.graph_scroll_pending,
                            open_menu,
                        })
                    } else {
                        None
                    };
                    let sel = app.selected_commit.as_ref().map(|(o, _)| *o);
                    let sel_file = app.selected_commit_file.clone();
                    let show_author = app.config.graph_show_author;
                    let show_date = app.config.graph_show_date;
                    let files_tree = app.config.graph_files_tree;
                    let mut clicked = None;
                    egui::ScrollArea::both()
                        .id_salt("graph")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            clicked = graph_view::draw(
                                &app.graph,
                                ui,
                                sel,
                                &app.commit_files,
                                &app.commit_detail,
                                sel_file.as_deref(),
                                show_author,
                                show_date,
                                files_tree,
                                &app.commit_folds,
                                cursor.as_ref(),
                                &mut app.graph_menu,
                            );
                        });
                    app.graph_scroll_pending = false;
                    match clicked {
                        Some(graph_view::GraphAction::Commit(oid)) => {
                            app.select_commit(oid);
                            app.set_graph_cursor_to_commit(oid);
                        }
                        Some(graph_view::GraphAction::File(path)) => {
                            app.select_commit_file(path.clone());
                            app.set_graph_cursor_to_file(&path);
                        }
                        Some(graph_view::GraphAction::ToggleFolder(path)) => {
                            app.toggle_commit_fold(path.clone());
                            app.set_graph_cursor_to_folder(&path);
                        }
                        Some(graph_view::GraphAction::RebaseOnto(oid)) => {
                            app.confirm_op = Some((crate::app::GraphOp::RebaseOnto, oid))
                        }
                        Some(graph_view::GraphAction::InteractiveRebase(oid)) => {
                            app.interactive_rebase(oid)
                        }
                        Some(graph_view::GraphAction::Amend) => app.begin_amend_from_graph(),
                        Some(graph_view::GraphAction::CherryPick(oid)) => {
                            app.confirm_op = Some((crate::app::GraphOp::CherryPick, oid))
                        }
                        Some(graph_view::GraphAction::Revert(oid)) => {
                            app.confirm_op = Some((crate::app::GraphOp::Revert, oid))
                        }
                        Some(graph_view::GraphAction::Switch(name)) => app.switch_branch(name),
                        Some(graph_view::GraphAction::CheckoutRemote(name)) => {
                            app.checkout_tracking(name)
                        }
                        Some(graph_view::GraphAction::DeleteRemoteBranch(name)) => {
                            app.confirm_delete = Some(crate::app::DeleteTarget::RemoteBranch(name))
                        }
                        Some(graph_view::GraphAction::CheckoutCommit(oid)) => {
                            app.confirm_op = Some((crate::app::GraphOp::Checkout, oid))
                        }
                        Some(graph_view::GraphAction::CreateBranch(oid)) => {
                            app.begin_create_branch(oid)
                        }
                        Some(graph_view::GraphAction::RenameBranch(name)) => {
                            app.begin_rename_branch(name)
                        }
                        Some(graph_view::GraphAction::DeleteBranch(name)) => {
                            app.confirm_delete = Some(crate::app::DeleteTarget::Branch(name))
                        }
                        Some(graph_view::GraphAction::CreateTag(oid)) => app.begin_create_tag(oid),
                        Some(graph_view::GraphAction::DeleteTag(name)) => {
                            app.confirm_delete = Some(crate::app::DeleteTarget::Tag(name))
                        }
                        Some(graph_view::GraphAction::OpenReset(oid)) => {
                            app.reset_prompt = Some(oid);
                        }
                        Some(graph_view::GraphAction::StashPop(i)) => app.stash_pop(i),
                        Some(graph_view::GraphAction::StashApply(i)) => app.stash_apply(i),
                        Some(graph_view::GraphAction::StashDrop(i)) => app.stash_drop(i),
                        Some(graph_view::GraphAction::Push) => app.push(&ctx, false),
                        Some(graph_view::GraphAction::ForcePush) => app.request_force_push(),
                        Some(graph_view::GraphAction::Fetch) => app.fetch(&ctx),
                        None => {}
                    }
                }
                Tab::Diff => {
                    let file_sel = app.selected_file.clone();
                    let conflict = app.diff.conflict;
                    let rename = app.diff.rename;
                    if let Some((path, staged)) = file_sel.clone() {
                        ui.horizontal(|ui| {
                            if ui.button("\u{e7c5}  Open in Neovim").clicked() {
                                app.open_in_editor(&path);
                            }
                            if conflict {
                                ui.colored_label(
                                    egui::Color32::from_rgb(0xff, 0xb8, 0x6c),
                                    "\u{f071}  Conflict — resolve in the editor, then stage",
                                );
                            } else if rename {
                                ui.colored_label(
                                    egui::Color32::from_rgb(0x6c, 0x9c, 0xff),
                                    "\u{f0c5}  Renamed — stage or discard the whole file",
                                );
                            } else if let Some((lo, hi)) = app.diff_highlight() {
                                let n = hi.saturating_sub(lo) + 1;
                                let label = if staged {
                                    format!("\u{f0e2}  Unstage lines ({n})")
                                } else {
                                    format!("\u{f067}  Stage lines ({n})")
                                };
                                if ui.button(label).clicked() {
                                    app.apply_line_selection();
                                }
                                if !staged
                                    && ui
                                        .add(
                                            egui::Button::new(format!(
                                                "\u{f0e2}  Discard lines ({n})"
                                            ))
                                            .fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)),
                                        )
                                        .clicked()
                                {
                                    app.request_discard_selection();
                                }
                                if ui.button("Clear").clicked() {
                                    app.diff_nav.anchor = None;
                                }
                            }
                            ui.weak(&path);
                        });
                        ui.separator();
                    } else if let Some((oid, label)) = app.selected_commit.clone() {
                        ui.horizontal(|ui| {
                            if oid.is_zero() {
                                ui.weak(label);
                            } else {
                                ui.weak(format!("commit {label}"));
                            }
                            if let Some(f) = &app.selected_commit_file {
                                ui.weak("·");
                                ui.weak(f);
                            }
                        });
                        ui.separator();
                    }

                    if file_sel.is_some() && app.find.open {
                        find_bar(app, ui);
                    }

                    let find_render = if file_sel.is_some() && app.find.open {
                        app.find.recompute(&app.diff);
                        let mut fr = diff_view::FindRender::default();
                        for (i, m) in app.find.matches.iter().enumerate() {
                            fr.rows.entry(m.row).or_default().push((
                                m.start,
                                m.end,
                                i == app.find.current,
                            ));
                        }
                        Some(fr)
                    } else {
                        None
                    };

                    let hunk_ctl = if conflict || rename {
                        None
                    } else {
                        file_sel.as_ref().map(|(_, staged)| *staged)
                    };
                    let nav = file_sel.as_ref().map(|_| diff_view::DiffNav {
                        cursor: app.diff_nav.cursor.min(app.diff_last_row()),
                        sel: app.diff_highlight(),
                        scroll_to_cursor: app.diff_scroll_pending,
                        center: app.diff_scroll_center,
                    });
                    app.ensure_diff_highlight(ui.visuals().dark_mode);
                    let diff_ver = app.diff_version();
                    let resp = diff_view::draw(
                        &app.diff,
                        ui,
                        hunk_ctl,
                        nav.as_ref(),
                        find_render.as_ref(),
                        &mut app.diff_hl,
                        &mut app.diff_galleys,
                        diff_ver,
                    );
                    app.diff_scrolled_prev = app.diff_scroll_pending;
                    app.diff_scroll_pending = false;
                    app.diff_scroll_center = false;
                    if nav.is_some() {
                        app.diff_visible = resp.visible;
                    }
                    if let Some(idx) = resp.hunk_toggle {
                        app.toggle_hunk(idx);
                    }
                    if let Some((a, c)) = resp.drag_select {
                        app.diff_nav.anchor = Some(a);
                        app.set_diff_cursor(c);
                        app.diff_scroll_pending = false;
                        app.focus = Pane::RightTab;
                    }
                }
                Tab::Editor => {
                    if app.term.as_mut().is_some_and(|t| !t.is_alive()) {
                        app.term = None;
                    }
                    if app.term.is_none() {
                        match crate::term::Term::spawn(
                            &app.nvim_socket,
                            &app.selected,
                            ui.ctx(),
                            app.repaint_gate(),
                        ) {
                            Ok(t) => app.term = Some(t),
                            Err(e) => app.error = Some(e),
                        }
                    }
                    if app.flush_pending_open() {
                        ui.ctx().request_repaint();
                    }
                    let active = app.focus == Pane::RightTab;
                    if let Some(t) = &mut app.term
                        && t.ui(ui, active)
                    {
                        app.focus = Pane::RightTab;
                    }
                }
                Tab::Search => {
                    if let Some(search_view::SearchAction::OpenEditor { path, line }) =
                        search_view::draw(app, ui)
                    {
                        app.open_in_editor_at(&path, line);
                    }
                }
            }
            focus_on_click(app, ui, Pane::RightTab);
        })
        .response
        .rect;
    pane_border(ui, central_rect, app.focus == Pane::RightTab);

    if let Some(req) = app.confirm_discard.clone() {
        let resp = egui::Modal::new(egui::Id::new("confirm_discard")).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.heading("Discard changes");
            ui.add_space(6.0);
            ui.label(format!(
                "Discard unstaged changes to {}. Staged changes are kept. Are you sure?",
                req.label
            ));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let cancel = ui.button("Cancel").clicked();
                let discard = ui
                    .add(
                        egui::Button::new("Discard")
                            .fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)),
                    )
                    .clicked();
                (discard, cancel)
            })
            .inner
        });
        let (discard, cancel) = resp.inner;
        if discard {
            app.discard_paths(&req.paths);
            app.confirm_discard = None;
        } else if cancel || resp.should_close() {
            app.confirm_discard = None;
        }
    }

    if let Some((path, lo, hi)) = app.confirm_discard_range.clone() {
        let n = hi.saturating_sub(lo) + 1;
        let resp = egui::Modal::new(egui::Id::new("confirm_discard_range")).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.heading("Discard lines");
            ui.add_space(6.0);
            ui.label(format!(
                "Discard the selected {n} line(s) from the working tree in {path}. Are you sure?"
            ));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let cancel = ui.button("Cancel").clicked();
                let discard = ui
                    .add(
                        egui::Button::new("Discard")
                            .fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)),
                    )
                    .clicked();
                (discard, cancel)
            })
            .inner
        });
        let (discard, cancel) = resp.inner;
        if discard {
            app.discard_line_selection(&path, lo, hi);
            app.confirm_discard_range = None;
        } else if cancel || resp.should_close() {
            app.confirm_discard_range = None;
        }
    }

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

fn help_key(app: &mut App, ui: &mut egui::Ui) {
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

fn draw_help(app: &mut App, ctx: &egui::Context) {
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

fn help_section(ui: &mut egui::Ui, keymap: &crate::keys::Keymap, ctx: crate::keys::Context) {
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

fn ref_prompt_modal(app: &mut App, ui: &mut egui::Ui) {
    use crate::app::RefPrompt;

    let (title, primary, with_switch, hint) = match &app.ref_prompt {
        Some(RefPrompt::CreateBranch { .. }) => ("Create branch", "Create", true, "branch name"),
        Some(RefPrompt::RenameBranch { .. }) => ("Rename branch", "Rename", false, "branch name"),
        Some(RefPrompt::CreateTag { .. }) => ("Create tag", "Create", false, "tag name"),
        None => return,
    };
    enum Done {
        Apply(bool),
        Cancel,
        Idle,
    }
    let resp = egui::Modal::new(egui::Id::new("ref_prompt")).show(ui.ctx(), |ui| {
        ui.set_width(320.0);
        ui.heading(title);
        ui.add_space(6.0);
        let edit = ui.add(
            egui::TextEdit::singleline(&mut app.name_input)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        );
        if app.name_input_focus {
            edit.request_focus();
            app.name_input_focus = false;
        }
        let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                return Done::Cancel;
            }
            if ui.button(primary).clicked() || submit {
                return Done::Apply(false);
            }
            if with_switch && ui.button("Create + Switch").clicked() {
                return Done::Apply(true);
            }
            Done::Idle
        })
        .inner
    });
    match resp.inner {
        Done::Apply(switch) => app.commit_ref_prompt(switch),
        Done::Cancel => app.ref_prompt = None,
        Done::Idle => {
            if resp.should_close() {
                app.ref_prompt = None;
            }
        }
    }
}

fn delete_ref_modal(app: &mut App, ui: &mut egui::Ui) {
    use crate::app::DeleteTarget;
    let Some(target) = &app.confirm_delete else {
        return;
    };
    let (kind, name) = match target {
        DeleteTarget::Branch(n) => ("branch", n.clone()),
        DeleteTarget::Tag(n) => ("tag", n.clone()),
        DeleteTarget::RemoteBranch(n) => ("remote branch", n.clone()),
    };
    let warn = if matches!(target, DeleteTarget::RemoteBranch(_)) {
        format!("Delete {kind} \"{name}\" on the remote? This cannot be undone.")
    } else {
        format!("Delete {kind} \"{name}\"? This cannot be undone from the UI.")
    };
    let ctx = ui.ctx().clone();
    let resp = egui::Modal::new(egui::Id::new("confirm_delete_ref")).show(ui.ctx(), |ui| {
        ui.set_width(340.0);
        ui.heading(format!("Delete {kind}"));
        ui.add_space(6.0);
        ui.label(warn);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel").clicked();
            let del = ui
                .add(egui::Button::new("Delete").fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)))
                .clicked();
            (del, cancel)
        })
        .inner
    });
    let (del, cancel) = resp.inner;
    if del {
        match app.confirm_delete.take().unwrap() {
            DeleteTarget::RemoteBranch(name) => app.delete_remote_branch(&ctx, name),
            other => app.delete_ref(&other),
        }
    } else if cancel || resp.should_close() {
        app.confirm_delete = None;
    }
}

fn find_bar(app: &mut App, ui: &mut egui::Ui) {
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

fn search_confirm_modal(app: &mut App, ui: &mut egui::Ui) {
    if !app.search_confirm {
        return;
    }
    let matches = app.search.selected_count();
    let files = app
        .search
        .results
        .iter()
        .filter(|f| {
            f.lines
                .iter()
                .any(|l| app.search.selected.contains(&(f.path.clone(), l.line_no)))
        })
        .count();
    let resp = egui::Modal::new(egui::Id::new("search_confirm")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading("Replace in working tree");
        ui.add_space(6.0);
        ui.label(format!(
            "Replace {matches} match(es) across {files} file(s). This edits files on disk and cannot be undone from the UI."
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel").clicked();
            let apply = ui
                .add(egui::Button::new("Replace").fill(egui::Color32::from_rgb(0x2e, 0x6b, 0x3a)))
                .clicked();
            (apply, cancel)
        })
        .inner
    });
    let (apply, cancel) = resp.inner;
    if apply {
        app.search_apply();
    } else if cancel || resp.should_close() {
        app.search_confirm = false;
    }
}

fn reset_modal(app: &mut App, ui: &mut egui::Ui) {
    use twit_core::repo::ResetMode;
    let Some(oid) = app.reset_prompt else {
        return;
    };
    let short = oid.to_string();

    let mut chosen: Option<ResetMode> = None;
    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::S) {
            chosen = Some(ResetMode::Soft);
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::M) {
            chosen = Some(ResetMode::Mixed);
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::H) {
            chosen = Some(ResetMode::Hard);
        }
    });

    enum Pick {
        Mode(ResetMode),
        Cancel,
        Idle,
    }
    let resp = egui::Modal::new(egui::Id::new("reset_prompt")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading(format!(
            "Reset current branch to {}",
            &short[..7.min(short.len())]
        ));
        ui.add_space(6.0);
        ui.label("Choose how to move the current branch (HEAD):");
        ui.add_space(8.0);
        let mut pick = Pick::Idle;
        if ui
            .button("Soft (s)  \u{2014} keep index and working tree")
            .clicked()
        {
            pick = Pick::Mode(ResetMode::Soft);
        }
        if ui
            .button("Mixed (m)  \u{2014} reset index, keep working tree")
            .clicked()
        {
            pick = Pick::Mode(ResetMode::Mixed);
        }
        if ui
            .add(
                egui::Button::new("Hard (h)  \u{2014} DISCARD working tree changes")
                    .fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)),
            )
            .clicked()
        {
            pick = Pick::Mode(ResetMode::Hard);
        }
        ui.add_space(8.0);
        if ui.button("Cancel (Esc)").clicked() {
            pick = Pick::Cancel;
        }
        pick
    });

    if let Pick::Mode(m) = &resp.inner {
        chosen = Some(*m);
    }
    if let Some(m) = chosen {
        app.do_reset(oid, m);
        app.reset_prompt = None;
    } else if matches!(resp.inner, Pick::Cancel) || resp.should_close() {
        app.reset_prompt = None;
    }
}

fn confirm_op_modal(app: &mut App, ui: &mut egui::Ui) {
    let Some((op, oid)) = app.confirm_op else {
        return;
    };
    let short = oid.to_string();
    let confirm_key = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let resp = egui::Modal::new(egui::Id::new("confirm_graph_op")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading(format!("{} {}", op.title(), &short[..7.min(short.len())]));
        ui.add_space(6.0);
        ui.label(op.detail());
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel (Esc)").clicked();
            let go = ui.button("Confirm (Enter)").clicked();
            (go, cancel)
        })
        .inner
    });
    let (go, cancel) = resp.inner;
    if go || confirm_key {
        app.run_confirmed_op();
    } else if cancel || resp.should_close() {
        app.confirm_op = None;
    }
}

fn force_push_modal(app: &mut App, ui: &mut egui::Ui) {
    if !app.confirm_force_push {
        return;
    }
    let ctx = ui.ctx().clone();
    let remote = twit_core::repo::primary_remote(&app.selected).unwrap_or_else(|| "origin".to_string());
    let branch = twit_core::repo::head_push_refspec(&app.selected)
        .and_then(|r| r.split(':').next().map(str::to_string))
        .map(|r| r.trim_start_matches("refs/heads/").to_string())
        .unwrap_or_default();
    let confirm_key = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let resp = egui::Modal::new(egui::Id::new("confirm_force_push")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading("\u{f0aa}  Force push");
        ui.add_space(6.0);
        ui.label(format!(
            "Force-push \"{branch}\" to \"{remote}\"? This overwrites the remote branch and can \
             discard commits others may have pushed."
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel (Esc)").clicked();
            let go = ui
                .add(
                    egui::Button::new("Force push (Enter)")
                        .fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)),
                )
                .clicked();
            (go, cancel)
        })
        .inner
    });
    let (go, cancel) = resp.inner;
    if go || confirm_key {
        app.confirm_force_push = false;
        app.push(&ctx, true);
    } else if cancel || resp.should_close() {
        app.confirm_force_push = false;
    }
}

fn amend_confirm_modal(app: &mut App, ui: &mut egui::Ui) {
    if !app.confirm_amend {
        return;
    }
    let confirm_key = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let resp = egui::Modal::new(egui::Id::new("confirm_amend")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading("Amend pushed commit");
        ui.add_space(6.0);
        ui.label(
            "HEAD matches its upstream. Amending rewrites a commit that already exists on the \
             remote — you will need to force-push afterward.",
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel (Esc)").clicked();
            let go = ui.button("Amend (Enter)").clicked();
            (go, cancel)
        })
        .inner
    });
    let (go, cancel) = resp.inner;
    if go || confirm_key {
        app.run_amend();
    } else if cancel || resp.should_close() {
        app.confirm_amend = false;
    }
}

struct CommitGuide {
    subject_len: usize,
    line2_nonblank: bool,
    body_over_72: bool,
}

fn commit_guide(msg: &str) -> CommitGuide {
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

fn commit_guide_row(app: &App, ui: &mut egui::Ui) {
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
                egui::Label::new(egui::RichText::new("72+").small().color(warn))
                    .selectable(false),
            )
            .on_hover_text("Body has lines longer than 72 columns");
        }
    });
}

fn draw_settings(app: &mut App, ctx: &egui::Context) {
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

enum StashAction {
    Pop(usize),
    Apply(usize),
    Drop(usize),
}

fn draw_stashes(app: &App, ui: &mut egui::Ui) -> Option<StashAction> {
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

fn remote_bar(app: &mut App, ui: &mut egui::Ui) {
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

fn rebase_banner(app: &mut App, ui: &mut egui::Ui) {
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

fn handle_global_keys(app: &mut App, ui: &mut egui::Ui) {
    use crate::keys::{Action, Context, KeymapPoll};

    if app.focus == Pane::Terminal && !app.shell_open {
        app.focus = Pane::RightTab;
    }

    let tab_cycles = app.focus == Pane::RightTab && app.active_tab != Tab::Editor;
    let right_tab_focus = app.focus == Pane::RightTab;
    let term_focus = app.terminal_focused();
    let actions = app
        .keymap
        .poll(ui, Context::Global, &mut app.pending_prefix, |a| match a {
            Action::CycleTab => tab_cycles,
            Action::CycleTabFwd | Action::CycleTabBack => right_tab_focus,
            Action::ToggleShell => !term_focus,
            _ => true,
        });

    let mut moved = false;
    for a in actions {
        match a {
            Action::FocusLeft => {
                app.move_focus(Dir::Left);
                moved = true;
            }
            Action::FocusRight => {
                app.move_focus(Dir::Right);
                moved = true;
            }
            Action::FocusUp => {
                app.move_focus(Dir::Up);
                moved = true;
            }
            Action::FocusDown => {
                app.move_focus(Dir::Down);
                moved = true;
            }
            Action::CycleTab | Action::CycleTabFwd => {
                app.focus = Pane::RightTab;
                app.active_tab = match app.active_tab {
                    Tab::Graph => Tab::Diff,
                    Tab::Diff => Tab::Search,
                    Tab::Search => Tab::Editor,
                    Tab::Editor => Tab::Graph,
                };
            }
            Action::CycleTabBack => {
                app.focus = Pane::RightTab;
                app.active_tab = match app.active_tab {
                    Tab::Graph => Tab::Editor,
                    Tab::Editor => Tab::Search,
                    Tab::Search => Tab::Diff,
                    Tab::Diff => Tab::Graph,
                };
            }
            Action::ToggleShell => app.toggle_shell(),
            Action::OpenSearch => {
                app.active_tab = Tab::Search;
                app.focus = Pane::RightTab;
                app.search.focus_request = true;
            }
            Action::NavBack => app.nav_go_back(),
            Action::NavForward => app.nav_go_forward(),
            _ => {}
        }
    }

    let (mouse_back, mouse_fwd) = ui.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Extra1),
            i.pointer.button_pressed(egui::PointerButton::Extra2),
        )
    });
    if mouse_back {
        app.nav_go_back();
    }
    if mouse_fwd {
        app.nav_go_forward();
    }

    if moved && let Some(id) = ui.ctx().memory(|m| m.focused()) {
        ui.ctx().memory_mut(|m| m.surrender_focus(id));
    }

    if moved && app.terminal_focused() {
        ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_))));
    }
}

fn diff_keys(app: &mut App, ui: &mut egui::Ui) {
    use crate::keys::{Action, Context, KeymapPoll};

    if app.focus != Pane::RightTab
        || app.active_tab != Tab::Diff
        || app.selected_file.is_none()
        || app.confirm_discard.is_some()
        || app.confirm_discard_range.is_some()
        || ui.ctx().memory(|m| m.focused().is_some())
    {
        return;
    }
    let last = app.diff_last_row();
    let staged = app.selected_file.as_ref().map(|(_, s)| *s).unwrap_or(false);
    let conflict = app.diff.conflict;

    let copy_event = ui.input_mut(|i| {
        let had = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
        i.events.retain(|e| !matches!(e, egui::Event::Copy));
        had
    });
    if copy_event && let Some(text) = app.diff_selection_text() {
        ui.ctx().copy_text(text);
        app.diff_nav.anchor = None;
    }

    let actions = app
        .keymap
        .poll(ui, Context::Diff, &mut app.pending_prefix, |_| true);
    for a in actions {
        match a {
            Action::DiffFind => app.toggle_find(),
            Action::DiffDown => app.move_diff_cursor(1),
            Action::DiffUp => app.move_diff_cursor(-1),
            Action::DiffTop => app.set_diff_cursor(0),
            Action::DiffBottom => app.set_diff_cursor(last),
            Action::DiffNextHunk => app.jump_hunk(true),
            Action::DiffPrevHunk => app.jump_hunk(false),
            Action::DiffToggleVisual => app.toggle_diff_visual(),
            Action::DiffClearVisual => app.diff_nav.anchor = None,
            Action::DiffStageSelection => {
                if !staged && !conflict {
                    app.apply_line_selection();
                }
            }
            Action::DiffUnstageSelection => {
                if staged && !conflict {
                    app.apply_line_selection();
                }
            }
            Action::DiffDiscardSelection => {
                if !staged && !conflict {
                    app.request_discard_selection();
                }
            }
            Action::DiffStageHunk => {
                if !staged
                    && !conflict
                    && let Some(h) = app.hunk_index_at_cursor()
                {
                    app.toggle_hunk(h);
                }
            }
            Action::DiffUnstageHunk => {
                if staged
                    && !conflict
                    && let Some(h) = app.hunk_index_at_cursor()
                {
                    app.toggle_hunk(h);
                }
            }
            Action::DiffHalfPageDown => app.scroll_diff(0.5, true),
            Action::DiffHalfPageUp => app.scroll_diff(0.5, false),
            Action::DiffPageDown => app.scroll_diff(1.0, true),
            Action::DiffPageUp => app.scroll_diff(1.0, false),
            Action::DiffEditor => {
                if let Some((path, _)) = app.selected_file.clone() {
                    app.open_in_editor(&path);
                }
            }
            Action::DiffCopySelection => {
                if let Some(text) = app.diff_selection_text() {
                    ui.ctx().copy_text(text);
                    app.diff_nav.anchor = None;
                }
            }
            _ => {}
        }
    }
}

fn graph_keys(app: &mut App, ui: &mut egui::Ui) -> bool {
    use crate::keys::{Action, Context, KeymapPoll};

    if app.graph_menu.is_some() {
        return false;
    }

    if app.help_open || app.any_modal_open() || ui.ctx().memory(|m| m.focused().is_some()) {
        return false;
    }

    app.clamp_graph_cursor();
    let ctx = ui.ctx().clone();
    let page = crate::app::LIST_PAGE as isize;
    let mut open_menu = false;
    let actions = app
        .keymap
        .poll(ui, Context::Graph, &mut app.pending_prefix, |_| true);
    for a in actions {
        match a {
            Action::GraphDown => app.move_graph_cursor(1),
            Action::GraphUp => app.move_graph_cursor(-1),
            Action::GraphTop => app.set_graph_cursor(0),
            Action::GraphBottom => app.graph_cursor_bottom(),
            Action::GraphHalfPageDown => app.move_graph_cursor(page),
            Action::GraphHalfPageUp => app.move_graph_cursor(-page),
            Action::GraphOpen => app.graph_activate(),
            Action::GraphEditor => app.graph_open_editor(),
            Action::GraphCollapse => app.graph_collapse(),
            Action::GraphContextMenu => open_menu = true,
            Action::GraphReset => {
                if let Some(oid) = app.graph_target_commit() {
                    app.reset_prompt = Some(oid);
                }
            }
            Action::GraphCreateBranch => {
                if let Some(oid) = app.graph_target_commit() {
                    app.begin_create_branch(oid);
                }
            }
            Action::GraphCreateTag => {
                if let Some(oid) = app.graph_target_commit() {
                    app.begin_create_tag(oid);
                }
            }
            Action::GraphCherryPick => {
                if let Some(oid) = app.graph_target_commit() {
                    app.confirm_op = Some((crate::app::GraphOp::CherryPick, oid));
                }
            }
            Action::GraphRevert => {
                if let Some(oid) = app.graph_target_commit() {
                    app.confirm_op = Some((crate::app::GraphOp::Revert, oid));
                }
            }
            Action::GraphRebaseOnto => {
                if let Some(oid) = app.graph_target_commit() {
                    app.confirm_op = Some((crate::app::GraphOp::RebaseOnto, oid));
                }
            }
            Action::GraphCheckout => {
                if let Some(oid) = app.graph_target_commit() {
                    app.confirm_op = Some((crate::app::GraphOp::Checkout, oid));
                }
            }
            Action::GraphRebaseInteractive => {
                if let Some(oid) = app.graph_target_commit() {
                    app.interactive_rebase(oid);
                }
            }
            Action::GraphPush => app.push(&ctx, false),
            Action::GraphForcePush => app.request_force_push(),
            Action::GraphFetch => app.fetch(&ctx),
            Action::GraphPull => app.pull(&ctx),
            _ => {}
        }
    }
    open_menu
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

#[derive(Default)]
struct TreeDir {
    dirs: BTreeMap<String, TreeDir>,
    files: Vec<Leaf>,
}

struct Leaf {
    name: String,
    path: String,
    old_path: Option<String>,
    kind: StatusKind,
}

fn build_tree(entries: &[StatusEntry]) -> TreeDir {
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

struct NavRow {
    depth: usize,
    staged: bool,
    kind: NavKind,
}

enum NavKind {
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

fn dir_id(salt: &str) -> egui::Id {
    egui::Id::new(("nav_dir", salt))
}
fn dir_label(salt: &str) -> String {
    salt.split_once('/')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| salt.to_string())
}
fn dir_open(ctx: &egui::Context, salt: &str) -> bool {
    ctx.data(|d| d.get_temp::<bool>(dir_id(salt)).unwrap_or(true))
}
fn set_dir_open(ctx: &egui::Context, salt: &str, open: bool) {
    ctx.data_mut(|d| d.insert_temp(dir_id(salt), open));
}

fn group_id(staged: bool) -> egui::Id {
    egui::Id::new(if staged {
        "nav_grp_staged"
    } else {
        "nav_grp_unstaged"
    })
}
fn group_open(ctx: &egui::Context, staged: bool) -> bool {
    ctx.data(|d| d.get_temp::<bool>(group_id(staged)).unwrap_or(true))
}
fn set_group_open(ctx: &egui::Context, staged: bool, open: bool) {
    ctx.data_mut(|d| d.insert_temp(group_id(staged), open));
}

fn build_nav_rows(
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

fn nav_paths(row: &NavRow) -> Vec<String> {
    match &row.kind {
        NavKind::File { path, old_path, .. } => file_paths(path, old_path.as_deref()),
        NavKind::Dir { paths, .. } => paths.clone(),
        NavKind::Group { paths, .. } => paths.clone(),
    }
}

fn file_paths(path: &str, old_path: Option<&str>) -> Vec<String> {
    match old_path {
        Some(old) if old != path => vec![old.to_string(), path.to_string()],
        _ => vec![path.to_string()],
    }
}

fn draw_changes(app: &mut App, ui: &mut egui::Ui) -> Option<Action> {
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

fn changes_nav(app: &mut App, ui: &mut egui::Ui, rows: &[NavRow]) -> Option<Action> {
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
fn render_rows(
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

fn render_group_row(
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
fn render_dir_row(
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

fn collect_paths(dir: &TreeDir) -> Vec<String> {
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
fn render_file_row(
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

fn marker_color(kind: StatusKind) -> egui::Color32 {
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
