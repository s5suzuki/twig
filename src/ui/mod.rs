pub mod diff_view;
mod graph_view;
mod search_view;
mod sidebar;

use std::collections::BTreeMap;

use crate::app::{App, Dir, Pane, Tab};
use crate::repo::{StatusEntry, StatusKind};

const BTN_W: f32 = 22.0;
const MARKER_W: f32 = 16.0;
const INDENT: f32 = 14.0;

enum Action {
    Select(String, bool),
    Stage(Vec<String>),
    Unstage(Vec<String>),
    OpenEditor(String),
    RequestDiscard(String),
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    handle_global_keys(app, ui);
    diff_keys(app, ui);

    app.ensure_watcher(ui.ctx());
    app.update_visibility(ui.ctx());
    app.poll_remote();
    if app.take_external_change() {
        app.refresh_from_disk();
    }

    let repos_rect = egui::Panel::left("repos")
        .default_size(200.0)
        .resizable(true)
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new("Repositories").heading()).truncate());
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
            egui::Panel::top("commit_box")
                .resizable(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::multiline(&mut app.commit_msg)
                            .hint_text("Commit message")
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        commit_clicked = ui
                            .button(format!("Commit ({} staged)", app.staged.len()))
                            .clicked();
                        stash_clicked = ui
                            .button("\u{f187}  Stash")
                            .on_hover_text("Stash all changes (incl. untracked)")
                            .clicked();
                    });
                    ui.add_space(4.0);
                });
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
                    Action::RequestDiscard(p) => {
                        if app.config.confirm_discard {
                            app.confirm_discard = Some(p);
                        } else {
                            app.discard_changes(&p);
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
                    && t.ui(ui, active) {
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
                    let sel = app.selected_commit.as_ref().map(|(o, _)| *o);
                    let sel_file = app.selected_commit_file.clone();
                    let show_author = app.config.graph_show_author;
                    let show_date = app.config.graph_show_date;
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
                            );
                        });
                    match clicked {
                        Some(graph_view::GraphAction::Commit(oid)) => app.select_commit(oid),
                        Some(graph_view::GraphAction::File(path)) => app.select_commit_file(path),
                        Some(graph_view::GraphAction::RebaseOnto(oid)) => app.rebase_onto(oid),
                        Some(graph_view::GraphAction::InteractiveRebase(oid)) => {
                            app.interactive_rebase(oid)
                        }
                        Some(graph_view::GraphAction::CherryPick(oid)) => app.cherry_pick(oid),
                        Some(graph_view::GraphAction::Revert(oid)) => app.revert(oid),
                        Some(graph_view::GraphAction::Switch(name)) => app.switch_branch(name),
                        Some(graph_view::GraphAction::CheckoutRemote(name)) => {
                            app.checkout_tracking(name)
                        }
                        Some(graph_view::GraphAction::DeleteRemoteBranch(name)) => {
                            app.confirm_delete = Some(crate::app::DeleteTarget::RemoteBranch(name))
                        }
                        Some(graph_view::GraphAction::CheckoutCommit(oid)) => {
                            app.checkout_commit(oid)
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
                        Some(graph_view::GraphAction::Reset(oid, mode)) => {
                            if mode == crate::repo::ResetMode::Hard {
                                app.confirm_reset = Some((oid, mode));
                            } else {
                                app.do_reset(oid, mode);
                            }
                        }
                        Some(graph_view::GraphAction::StashPop(i)) => app.stash_pop(i),
                        Some(graph_view::GraphAction::StashApply(i)) => app.stash_apply(i),
                        Some(graph_view::GraphAction::StashDrop(i)) => app.stash_drop(i),
                        None => {}
                    }
                }
                Tab::Diff => {
                    let file_sel = app.selected_file.clone();
                    let conflict = app.diff.conflict;
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
                                if ui.button("Clear").clicked() {
                                    app.diff_anchor = None;
                                }
                            }
                            ui.weak(&path);
                        });
                        ui.separator();
                    } else if let Some((_, label)) = app.selected_commit.clone() {
                        ui.horizontal(|ui| {
                            ui.weak(format!("commit {label}"));
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
                            fr.rows
                                .entry(m.row)
                                .or_default()
                                .push((m.start, m.end, i == app.find.current));
                        }
                        Some(fr)
                    } else {
                        None
                    };

                    let hunk_ctl = if conflict {
                        None
                    } else {
                        file_sel.as_ref().map(|(_, staged)| *staged)
                    };
                    let nav = file_sel.as_ref().map(|_| diff_view::DiffNav {
                        cursor: app.diff_cursor.min(app.diff_last_row()),
                        sel: app.diff_highlight(),
                        scroll_to_cursor: app.diff_scroll_pending,
                    });
                    app.ensure_diff_highlight(ui.visuals().dark_mode);
                    let diff_ver = app.diff_version();
                    let resp = diff_view::draw(
                        &app.diff,
                        ui,
                        hunk_ctl,
                        nav.as_ref(),
                        find_render.as_ref(),
                        &app.diff_hl,
                        &mut app.diff_galleys,
                        diff_ver,
                    );
                    app.diff_scrolled_prev = app.diff_scroll_pending;
                    app.diff_scroll_pending = false;
                    if nav.is_some() {
                        app.diff_visible = resp.visible;
                    }
                    if let Some(idx) = resp.hunk_toggle {
                        app.toggle_hunk(idx);
                    }
                    if let Some((a, c)) = resp.drag_select {
                        app.diff_anchor = Some(a);
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
                        && t.ui(ui, active) {
                            app.focus = Pane::RightTab;
                        }
                }
                Tab::Search => {
                    if let Some(search_view::SearchAction::OpenEditor(p)) =
                        search_view::draw(app, ui)
                    {
                        app.open_in_editor(&p);
                    }
                }
            }
            focus_on_click(app, ui, Pane::RightTab);
        })
        .response
        .rect;
    pane_border(ui, central_rect, app.focus == Pane::RightTab);

    if let Some(path) = app.confirm_discard.clone() {
        let resp = egui::Modal::new(egui::Id::new("confirm_discard")).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.heading("Discard changes");
            ui.add_space(6.0);
            ui.label(format!(
                "Discard changes to {path} and restore it to HEAD. Are you sure?"
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
            app.discard_changes(&path);
            app.confirm_discard = None;
        } else if cancel || resp.should_close() {
            app.confirm_discard = None;
        }
    }

    ref_prompt_modal(app, ui);
    delete_ref_modal(app, ui);
    reset_modal(app, ui);
    search_confirm_modal(app, ui);

    draw_settings(app, ui.ctx());
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
        edit.request_focus();
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
            if ui.button("\u{f062}").on_hover_text("Previous (Shift+Enter)").clicked() {
                go_prev = true;
            }
            if ui.button("\u{f063}").on_hover_text("Next (Enter)").clicked() {
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
    let Some((oid, mode)) = app.confirm_reset else {
        return;
    };
    let short = oid.to_string();
    let resp = egui::Modal::new(egui::Id::new("confirm_reset")).show(ui.ctx(), |ui| {
        ui.set_width(360.0);
        ui.heading(format!("{} reset", mode.label()));
        ui.add_space(6.0);
        ui.label(format!(
            "Move the current branch to {} and DISCARD all uncommitted changes in the working tree. This cannot be undone.",
            &short[..7.min(short.len())]
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel").clicked();
            let go = ui
                .add(egui::Button::new(format!("{} reset", mode.label()))
                    .fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)))
                .clicked();
            (go, cancel)
        })
        .inner
    });
    let (go, cancel) = resp.inner;
    if go {
        app.do_reset(oid, mode);
        app.confirm_reset = None;
    } else if cancel || resp.should_close() {
        app.confirm_reset = None;
    }
}

fn draw_settings(app: &mut App, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    use crate::config::{Accent, Theme};
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
                if ui
                    .button("\u{f0aa}  Force push (current branch)")
                    .clicked()
                {
                    app.push(&ctx, true);
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
    use crate::keys::{Action, Context};

    if app.focus == Pane::Terminal && !app.shell_open {
        app.focus = Pane::RightTab;
    }

    let tab_cycles = app.focus == Pane::RightTab && app.active_tab != Tab::Editor;
    let term_focus = app.terminal_focused();
    let actions = app.keymap.poll(ui, Context::Global, &mut app.pending_prefix, |a| {
        match a {
            Action::CycleTab => tab_cycles,
            Action::ToggleShell => !term_focus,
            _ => true,
        }
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
            _ => {}
        }
    }
    if moved
        && let Some(id) = ui.ctx().memory(|m| m.focused())
    {
        ui.ctx().memory_mut(|m| m.surrender_focus(id));
    }
}

fn diff_keys(app: &mut App, ui: &mut egui::Ui) {
    use crate::keys::{Action, Context};

    if app.focus != Pane::RightTab
        || app.active_tab != Tab::Diff
        || app.selected_file.is_none()
        || app.confirm_discard.is_some()
        || ui.ctx().memory(|m| m.focused().is_some())
    {
        return;
    }
    let last = app.diff_last_row();
    let staged = app.selected_file.as_ref().map(|(_, s)| *s).unwrap_or(false);
    let conflict = app.diff.conflict;

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
            Action::DiffToggleVisual => app.toggle_diff_visual(),
            Action::DiffClearVisual => app.diff_anchor = None,
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
            Action::DiffHalfPageDown => app.scroll_diff(0.5, true),
            Action::DiffHalfPageUp => app.scroll_diff(0.5, false),
            Action::DiffPageDown => app.scroll_diff(1.0, true),
            Action::DiffPageUp => app.scroll_diff(1.0, false),
            _ => {}
        }
    }
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
        kind: StatusKind,
    },
}

fn dir_id(salt: &str) -> egui::Id {
    egui::Id::new(("nav_dir", salt))
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
                kind: leaf.kind,
            },
        });
    }
}

fn nav_paths(row: &NavRow) -> Vec<String> {
    match &row.kind {
        NavKind::File { path, .. } => vec![path.clone()],
        NavKind::Dir { paths, .. } => paths.clone(),
        NavKind::Group { paths, .. } => paths.clone(),
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
    render_rows(ui, &rows, 0, cursor, sel.as_ref(), &mut action);
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
        || app.confirm_discard.is_some()
        || ui.ctx().memory(|m| m.focused().is_some())
    {
        return None;
    }

    use crate::keys::{Action as Cmd, Context};
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
                    }) {
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
    if e
        && let NavKind::File { path, .. } = &cur.kind {
            app.focus = Pane::RightTab;
            action = Some(Action::OpenEditor(path.clone()));
        }
    if d && !cur.staged
        && let NavKind::File { path, .. } = &cur.kind {
            action = Some(Action::RequestDiscard(path.clone()));
        }
    action
}

fn render_rows(
    ui: &mut egui::Ui,
    rows: &[NavRow],
    base: usize,
    cursor: usize,
    sel: Option<&(String, bool)>,
    action: &mut Option<Action>,
) {
    for (i, row) in rows.iter().enumerate() {
        let is_cursor = base + i == cursor;
        match &row.kind {
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
            NavKind::File { name, path, kind } => render_file_row(
                ui, row.staged, name, path, *kind, row.depth, is_cursor, sel, action,
            ),
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
) {
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

    let btn = if staged { "−" } else { "+" };
    let btn_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - BTN_W - 2.0, rect.center().y - 9.0),
        egui::vec2(BTN_W, 18.0),
    );
    let mut btn_clicked = false;
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
) {
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
}

fn collect_paths(dir: &TreeDir) -> Vec<String> {
    let mut out = Vec::new();
    for sub in dir.dirs.values() {
        out.extend(collect_paths(sub));
    }
    for leaf in &dir.files {
        out.push(leaf.path.clone());
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_file_row(
    ui: &mut egui::Ui,
    staged: bool,
    name: &str,
    path: &str,
    kind: StatusKind,
    depth: usize,
    is_cursor: bool,
    sel: Option<&(String, bool)>,
    action: &mut Option<Action>,
) {
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
    ui.painter().text(
        egui::pos2(x + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
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
                *action = Some(Action::RequestDiscard(path.to_string()));
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
                Action::Unstage(vec![path.to_string()])
            } else {
                Action::Stage(vec![path.to_string()])
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
}

fn marker_color(kind: StatusKind) -> egui::Color32 {
    match kind {
        StatusKind::New => egui::Color32::from_rgb(0x7e, 0xe7, 0x87),
        StatusKind::Modified => egui::Color32::from_rgb(0xe6, 0xd8, 0x6b),
        StatusKind::Deleted => egui::Color32::from_rgb(0xff, 0x7b, 0x7b),
        StatusKind::Renamed => egui::Color32::from_rgb(0x6c, 0x9c, 0xff),
        StatusKind::Conflicted => egui::Color32::from_rgb(0xff, 0xb8, 0x6c),
        _ => egui::Color32::from_gray(150),
    }
}
