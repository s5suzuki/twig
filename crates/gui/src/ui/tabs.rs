use super::*;

pub(super) fn central_panel(app: &mut App, ui: &mut egui::Ui) {
    let rect = egui::CentralPanel::default()
        .show(ui, |ui| {
            tab_bar(app, ui);
            ui.separator();
            match app.active_tab {
                Tab::Graph => graph_tab(app, ui),
                Tab::Diff => diff_tab(app, ui),
                Tab::Editor => editor_tab(app, ui),
                Tab::Search => search_tab(app, ui),
            }
            focus_on_click(app, ui, Pane::RightTab);
        })
        .response
        .rect;
    pane_border(ui, rect, app.focus == Pane::RightTab);
}

fn tab_bar(app: &mut App, ui: &mut egui::Ui) {
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
}

fn graph_tab(app: &mut App, ui: &mut egui::Ui) {
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
    if let Some(action) = clicked {
        apply_graph_action(app, &ctx, action);
    }
}

fn apply_graph_action(app: &mut App, ctx: &egui::Context, action: graph_view::GraphAction) {
    use crate::app::{DeleteTarget, GraphOp};
    use graph_view::GraphAction;
    match action {
        GraphAction::Commit(oid) => {
            app.select_commit(oid);
            app.set_graph_cursor_to_commit(oid);
        }
        GraphAction::File(path) => {
            app.select_commit_file(path.clone());
            app.set_graph_cursor_to_file(&path);
        }
        GraphAction::ToggleFolder(path) => {
            app.toggle_commit_fold(path.clone());
            app.set_graph_cursor_to_folder(&path);
        }
        GraphAction::RebaseOnto(oid) => app.confirm_op = Some((GraphOp::RebaseOnto, oid)),
        GraphAction::InteractiveRebase(oid) => app.interactive_rebase(oid),
        GraphAction::Amend => app.begin_amend_from_graph(),
        GraphAction::CherryPick(oid) => app.confirm_op = Some((GraphOp::CherryPick, oid)),
        GraphAction::Revert(oid) => app.confirm_op = Some((GraphOp::Revert, oid)),
        GraphAction::Merge(oid) => app.confirm_op = Some((GraphOp::Merge, oid)),
        GraphAction::Switch(name) => app.switch_branch(name),
        GraphAction::CheckoutRemote(name) => app.checkout_tracking(name),
        GraphAction::DeleteRemoteBranch(name) => {
            app.confirm_delete = Some(DeleteTarget::RemoteBranch(name))
        }
        GraphAction::CheckoutCommit(oid) => app.confirm_op = Some((GraphOp::Checkout, oid)),
        GraphAction::CreateBranch(oid) => app.begin_create_branch(oid),
        GraphAction::RenameBranch(name) => app.begin_rename_branch(name),
        GraphAction::DeleteBranch(name) => app.confirm_delete = Some(DeleteTarget::Branch(name)),
        GraphAction::CreateTag(oid) => app.begin_create_tag(oid),
        GraphAction::DeleteTag(name) => app.confirm_delete = Some(DeleteTarget::Tag(name)),
        GraphAction::OpenReset(oid) => app.reset_prompt = Some(oid),
        GraphAction::StashPop(i) => app.stash_pop(i),
        GraphAction::StashApply(i) => app.stash_apply(i),
        GraphAction::StashDrop(i) => app.stash_drop(i),
        GraphAction::Push => app.push(ctx, false),
        GraphAction::ForcePush => app.request_force_push(),
        GraphAction::Fetch => app.fetch(ctx),
    }
}

fn diff_tab(app: &mut App, ui: &mut egui::Ui) {
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
                            egui::Button::new(format!("\u{f0e2}  Discard lines ({n})"))
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
            fr.rows
                .entry(m.row)
                .or_default()
                .push((m.start, m.end, i == app.find.current));
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

fn editor_tab(app: &mut App, ui: &mut egui::Ui) {
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

fn search_tab(app: &mut App, ui: &mut egui::Ui) {
    if let Some(search_view::SearchAction::OpenEditor { path, line }) = search_view::draw(app, ui) {
        app.open_in_editor_at(&path, line);
    }
}
