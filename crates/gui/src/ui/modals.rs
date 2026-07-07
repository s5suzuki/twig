use super::*;

pub(super) fn ref_prompt_modal(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn delete_ref_modal(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn search_confirm_modal(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn reset_modal(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn confirm_op_modal(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn force_push_modal(app: &mut App, ui: &mut egui::Ui) {
    if !app.confirm_force_push {
        return;
    }
    let ctx = ui.ctx().clone();
    let remote =
        twit_core::repo::primary_remote(&app.selected).unwrap_or_else(|| "origin".to_string());
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

pub(super) fn amend_confirm_modal(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn confirm_discard_modal(app: &mut App, ui: &mut egui::Ui) {
    let Some(req) = app.confirm_discard.clone() else {
        return;
    };
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
                .add(egui::Button::new("Discard").fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)))
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

pub(super) fn confirm_discard_range_modal(app: &mut App, ui: &mut egui::Ui) {
    let Some((path, lo, hi)) = app.confirm_discard_range.clone() else {
        return;
    };
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
                .add(egui::Button::new("Discard").fill(egui::Color32::from_rgb(0x8b, 0x2e, 0x2e)))
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
