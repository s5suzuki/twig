use super::*;

pub(super) fn handle_global_keys(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn diff_keys(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn graph_keys(app: &mut App, ui: &mut egui::Ui) -> bool {
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
