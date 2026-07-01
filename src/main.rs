mod app;
mod config;
mod editor;
mod fonts;
mod keys;
mod repo;
mod term;
mod ui;
mod watch;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--dump") => {
            let path = args
                .get(2)
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
            dump(&path);
            return Ok(());
        }
        Some("--diff") => {
            let path = PathBuf::from(&args[2]);
            let staged = args.get(4).map(String::as_str) == Some("staged");
            let mode = if staged {
                repo::DiffMode::Staged
            } else {
                repo::DiffMode::Unstaged
            };
            match repo::file_diff(&path, &args[3], mode) {
                Ok(d) => print_diff(&d),
                Err(e) => eprintln!("diff failed: {e}"),
            }
            return Ok(());
        }
        Some("--edit") => {
            let path = PathBuf::from(&args[2]);
            match editor::open(&path, &args[3]) {
                Ok(()) => println!("OK: edit"),
                Err(e) => eprintln!("{e}"),
            }
            return Ok(());
        }
        Some("--edit-server") => {
            let path = PathBuf::from(&args[2]);
            let socket = PathBuf::from(&args[4]);
            match editor::open_in_server(&path, &args[3], &socket) {
                Ok(()) => println!("OK: edit-server -> {}", socket.display()),
                Err(e) => eprintln!("{e}"),
            }
            return Ok(());
        }
        Some(op @ ("--stage-hunk" | "--unstage-hunk")) => {
            let path = PathBuf::from(&args[2]);
            let file = &args[3];
            let idx: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
            let res = if op == "--stage-hunk" {
                repo::stage_hunk(&path, file, idx)
            } else {
                repo::unstage_hunk(&path, file, idx)
            };
            match res {
                Ok(()) => {
                    println!("OK: {op} {file} #{idx}");
                    dump(&path);
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some(op @ ("--stage-lines" | "--unstage-lines")) => {
            let path = PathBuf::from(&args[2]);
            let file = &args[3];
            let lo: usize = args[4].parse().unwrap_or(0);
            let hi: usize = args[5].parse().unwrap_or(lo);
            let mode = if op == "--unstage-lines" {
                repo::DiffMode::Staged
            } else {
                repo::DiffMode::Unstaged
            };
            let res = repo::file_diff(&path, file, mode).and_then(|d| {
                repo::apply_partial(&path, file, &d.rows, lo, hi, op == "--unstage-lines")
            });
            match res {
                Ok(()) => {
                    println!("OK: {op} {file} rows {lo}..={hi}");
                    dump(&path);
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some("--selftest") => {
            selftest(&PathBuf::from(&args[2]), &args[3]);
            return Ok(());
        }
        Some("--commit-files") => {
            let path = PathBuf::from(&args[2]);
            match git2::Repository::open(&path).and_then(|r| Ok(r.revparse_single(&args[3])?.id()))
            {
                Ok(oid) => match repo::commit_files(&path, oid) {
                    Ok(files) => {
                        for f in &files {
                            println!("{} {}", f.kind.marker(), f.path);
                        }
                    }
                    Err(e) => eprintln!("commit-files failed: {e}"),
                },
                Err(e) => eprintln!("revparse failed: {e}"),
            }
            return Ok(());
        }
        Some("--commit-diff") => {
            let path = PathBuf::from(&args[2]);
            match git2::Repository::open(&path).and_then(|r| Ok(r.revparse_single(&args[3])?.id()))
            {
                Ok(oid) => match repo::commit_diff(&path, oid) {
                    Ok(d) => print_diff(&d),
                    Err(e) => eprintln!("commit-diff failed: {e}"),
                },
                Err(e) => eprintln!("revparse failed: {e}"),
            }
            return Ok(());
        }
        Some(op @ ("--cherry-pick" | "--cherry-pick-continue" | "--cherry-pick-abort")) => {
            let path = PathBuf::from(&args[2]);
            let res: Result<Option<repo::SeqOutcome>, git2::Error> = match op {
                "--cherry-pick-abort" => repo::cherry_pick_abort(&path).map(|()| None),
                "--cherry-pick-continue" => repo::cherry_pick_continue(&path).map(Some),
                _ => git2::Repository::open(&path)
                    .and_then(|r| Ok(r.revparse_single(&args[3])?.id()))
                    .and_then(|oid| repo::cherry_pick(&path, oid).map(Some)),
            };
            match res {
                Ok(Some(repo::SeqOutcome::Done)) | Ok(None) => {
                    println!("OK: {op}");
                    dump(&path);
                }
                Ok(Some(repo::SeqOutcome::Conflicts(files))) => {
                    println!("CONFLICTS: {} file(s):", files.len());
                    for f in &files {
                        println!("  {f}");
                    }
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some(op @ ("--revert" | "--revert-continue" | "--revert-abort")) => {
            let path = PathBuf::from(&args[2]);
            let res: Result<Option<repo::SeqOutcome>, git2::Error> = match op {
                "--revert-abort" => repo::revert_abort(&path).map(|()| None),
                "--revert-continue" => repo::revert_continue(&path).map(Some),
                _ => git2::Repository::open(&path)
                    .and_then(|r| Ok(r.revparse_single(&args[3])?.id()))
                    .and_then(|oid| repo::revert(&path, oid).map(Some)),
            };
            match res {
                Ok(Some(repo::SeqOutcome::Done)) | Ok(None) => {
                    println!("OK: {op}");
                    dump(&path);
                }
                Ok(Some(repo::SeqOutcome::Conflicts(files))) => {
                    println!("CONFLICTS: {} file(s):", files.len());
                    for f in &files {
                        println!("  {f}");
                    }
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some(
            op @ ("--stash" | "--stash-pop" | "--stash-apply" | "--stash-drop" | "--stash-list"),
        ) => {
            let path = PathBuf::from(&args[2]);
            let idx: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let res = match op {
                "--stash" => repo::stash_save(&path, args.get(3).map(String::as_str)),
                "--stash-pop" => repo::stash_pop(&path, idx),
                "--stash-apply" => repo::stash_apply(&path, idx),
                "--stash-drop" => repo::stash_drop(&path, idx),
                _ => {
                    for s in repo::stash_list(&path) {
                        println!("stash@{{{}}}: {}", s.index, s.message);
                    }
                    Ok(())
                }
            };
            match res {
                Ok(()) => {
                    if op != "--stash-list" {
                        println!("OK: {op}");
                        dump(&path);
                    }
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some(op @ ("--create-tag" | "--delete-tag")) => {
            let path = PathBuf::from(&args[2]);
            let res = if op == "--create-tag" {
                git2::Repository::open(&path)
                    .and_then(|r| Ok(r.revparse_single(&args[4])?.id()))
                    .and_then(|oid| repo::create_tag(&path, &args[3], oid))
            } else {
                repo::delete_tag(&path, &args[3])
            };
            match res {
                Ok(()) => {
                    println!("OK: {op}");
                    dump(&path);
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some("--reset") => {
            let path = PathBuf::from(&args[2]);
            let mode = match args.get(4).map(String::as_str) {
                Some("soft") => repo::ResetMode::Soft,
                Some("hard") => repo::ResetMode::Hard,
                _ => repo::ResetMode::Mixed,
            };
            match git2::Repository::open(&path).and_then(|r| Ok(r.revparse_single(&args[3])?.id()))
            {
                Ok(oid) => match repo::reset(&path, oid, mode) {
                    Ok(()) => {
                        println!("OK: reset {} {}", args[3], mode.label());
                        dump(&path);
                    }
                    Err(e) => eprintln!("reset failed: {e}"),
                },
                Err(e) => eprintln!("revparse failed: {e}"),
            }
            return Ok(());
        }
        Some(op @ ("--create-branch" | "--rename-branch" | "--delete-branch")) => {
            let path = PathBuf::from(&args[2]);
            let res = match op {
                "--create-branch" => git2::Repository::open(&path)
                    .and_then(|r| Ok(r.revparse_single(&args[4])?.id()))
                    .and_then(|oid| repo::create_branch(&path, &args[3], oid)),
                "--rename-branch" => repo::rename_branch(&path, &args[3], &args[4]),
                _ => repo::delete_branch(&path, &args[3]),
            };
            match res {
                Ok(()) => {
                    println!("OK: {op}");
                    dump(&path);
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some("--switch") => {
            let path = PathBuf::from(&args[2]);
            match repo::checkout_branch(&path, &args[3]) {
                Ok(()) => {
                    println!("OK: switch -> {}", args[3]);
                    dump(&path);
                }
                Err(e) => eprintln!("switch failed: {e}"),
            }
            return Ok(());
        }
        Some("--checkout") => {
            let path = PathBuf::from(&args[2]);
            match git2::Repository::open(&path).and_then(|r| Ok(r.revparse_single(&args[3])?.id()))
            {
                Ok(oid) => match repo::checkout_commit(&path, oid) {
                    Ok(()) => {
                        println!("OK: checkout (detached) -> {}", args[3]);
                        dump(&path);
                    }
                    Err(e) => eprintln!("checkout failed: {e}"),
                },
                Err(e) => eprintln!("revparse failed: {e}"),
            }
            return Ok(());
        }
        Some("--rebase-onto") => {
            let path = PathBuf::from(&args[2]);
            match git2::Repository::open(&path).and_then(|r| Ok(r.revparse_single(&args[3])?.id()))
            {
                Ok(oid) => match repo::rebase_onto(&path, oid) {
                    Ok(repo::SeqOutcome::Done) => {
                        println!("OK: rebase-onto {} (done)", &args[3]);
                        dump(&path);
                    }
                    Ok(repo::SeqOutcome::Conflicts(files)) => {
                        println!("CONFLICTS: rebase paused, {} file(s):", files.len());
                        for f in &files {
                            println!("  {f}");
                        }
                    }
                    Err(e) => eprintln!("rebase-onto failed: {e}"),
                },
                Err(e) => eprintln!("revparse failed: {e}"),
            }
            return Ok(());
        }
        Some(op @ ("--rebase-continue" | "--rebase-abort")) => {
            let path = PathBuf::from(&args[2]);
            if op == "--rebase-abort" {
                match repo::rebase_abort(&path) {
                    Ok(()) => {
                        println!("OK: rebase-abort");
                        dump(&path);
                    }
                    Err(e) => eprintln!("rebase-abort failed: {e}"),
                }
            } else {
                match repo::rebase_continue(&path) {
                    Ok(repo::SeqOutcome::Done) => {
                        println!("OK: rebase-continue (done)");
                        dump(&path);
                    }
                    Ok(repo::SeqOutcome::Conflicts(files)) => {
                        println!("CONFLICTS: rebase still paused, {} file(s):", files.len());
                        for f in &files {
                            println!("  {f}");
                        }
                    }
                    Err(e) => eprintln!("rebase-continue failed: {e}"),
                }
            }
            return Ok(());
        }
        Some("--remote-list") => {
            let path = PathBuf::from(&args[2]);
            for r in repo::remotes(&path) {
                println!("{}\t{}", r.name, r.url.unwrap_or_default());
            }
            return Ok(());
        }
        Some("--delete-remote") => {
            let path = PathBuf::from(&args[2]);
            match args[3].split_once('/') {
                Some((remote, branch)) => {
                    match repo::delete_remote_branch(&path, remote, branch, |_, _| {}) {
                        Ok(()) => {
                            println!("OK: --delete-remote {}", args[3]);
                            dump(&path);
                        }
                        Err(e) => eprintln!("--delete-remote failed: {e}"),
                    }
                }
                None => eprintln!("--delete-remote: expected <remote>/<branch>"),
            }
            return Ok(());
        }
        Some(op @ ("--fetch" | "--pull" | "--push")) => {
            let path = PathBuf::from(&args[2]);
            let remote = args
                .get(3)
                .cloned()
                .or_else(|| repo::primary_remote(&path))
                .unwrap_or_else(|| "origin".to_string());
            let progress = |_recv: usize, _total: usize| {};
            let res: Result<repo::SeqOutcome, git2::Error> = match op {
                "--fetch" => repo::fetch(&path, &remote, progress).map(|()| repo::SeqOutcome::Done),
                "--pull" => repo::pull(&path, &remote, progress),
                _ => match args.get(4).cloned().or_else(|| repo::head_push_refspec(&path)) {
                    Some(refspec) => {
                        repo::push(&path, &remote, std::slice::from_ref(&refspec), progress)
                            .map(|()| repo::SeqOutcome::Done)
                    }
                    None => Err(git2::Error::from_str("not on a branch; specify a refspec")),
                },
            };
            match res {
                Ok(repo::SeqOutcome::Done) => {
                    println!("OK: {op} {remote}");
                    dump(&path);
                }
                Ok(repo::SeqOutcome::Conflicts(files)) => {
                    println!("CONFLICTS: {} file(s):", files.len());
                    for f in &files {
                        println!("  {f}");
                    }
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        Some(op @ ("--stage" | "--unstage" | "--discard" | "--commit")) => {
            let path = PathBuf::from(&args[2]);
            let res = match op {
                "--stage" => repo::stage(&path, std::slice::from_ref(&args[3])),
                "--unstage" => repo::unstage(&path, std::slice::from_ref(&args[3])),
                "--discard" => repo::discard(&path, std::slice::from_ref(&args[3])),
                _ => repo::commit(&path, &args[3]),
            };
            match res {
                Ok(()) => {
                    println!("OK: {op}");
                    dump(&path);
                }
                Err(e) => eprintln!("{op} failed: {e}"),
            }
            return Ok(());
        }
        _ => {}
    }

    let path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "twig",
        native_options,
        Box::new(move |cc| {
            let app = app::App::new(path);
            fonts::install(&cc.egui_ctx, &app.config.mono_font);
            app.apply_config(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

fn selftest(path: &std::path::Path, file: &str) {
    use egui::{Event, Key, Modifiers, PointerButton};

    let mut app = app::App::new(path.to_path_buf());
    app.shell_open = false;
    app.select_file(file.to_string(), false);
    app.focus = app::Pane::RightTab;
    app.active_tab = app::Tab::Diff;

    let ctx = egui::Context::default();
    let run = |app: &mut app::App, events: Vec<Event>| {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        raw.events = events;
        ctx.begin_pass(raw);
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 800.0));
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("selftest_root"),
            egui::UiBuilder::new().max_rect(rect),
        );
        ui::draw(app, &mut ui);
        let _ = ctx.end_pass();
    };
    let report = |tag: &str, app: &app::App| {
        println!(
            "{tag:<16} cursor={} anchor={:?} visible={:?} rows={} sel={:?} err={:?}",
            app.diff_cursor,
            app.diff_anchor,
            app.diff_visible,
            app.diff.rows.len(),
            app.selected_file,
            app.error,
        );
    };

    run(&mut app, vec![]);
    report("frame1", &app);

    println!("\n-- scroll down (wheel) with pointer over diff --");
    run(
        &mut app,
        vec![Event::PointerMoved(egui::pos2(800.0, 200.0))],
    );
    for _ in 0..3 {
        run(
            &mut app,
            vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -1200.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }],
        );
    }
    report("after scroll", &app);

    println!("\n-- keyboard: press j (cursor 0 is now off-screen) --");
    run(
        &mut app,
        vec![
            Event::Key {
                key: Key::J,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Key {
                key: Key::J,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    report("after j", &app);
    run(&mut app, vec![]);
    report("idle after j", &app);

    println!("\n-- fast j x120 (one per frame), cursor must advance monotonically --");
    let mut prev = app.diff_cursor;
    let mut backward = 0;
    let mut min_seen = prev;
    let mut non_line = 0;
    for _ in 0..120 {
        run(
            &mut app,
            vec![
                Event::Key {
                    key: Key::J,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
                Event::Key {
                    key: Key::J,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        let c = app.diff_cursor;
        if c < prev {
            backward += 1;
            min_seen = min_seen.min(c);
        }
        if !matches!(app.diff.rows.get(c), Some(repo::DiffRow::Line { .. })) {
            non_line += 1;
        }
        prev = c;
    }
    println!(
        "final cursor={} backward_jumps={} non_line_landings={} (both 0 = good)  visible={:?}",
        app.diff_cursor, backward, non_line, app.diff_visible
    );

    let p1 = egui::pos2(800.0, 120.0);
    let p2 = egui::pos2(800.0, 760.0);
    println!("\n-- mouse: move->press @ {p1:?}, drag to {p2:?}, release --");
    run(&mut app, vec![Event::PointerMoved(p1)]);
    run(
        &mut app,
        vec![Event::PointerButton {
            pos: p1,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    report("press", &app);
    run(&mut app, vec![Event::PointerMoved(p2)]);
    report("drag", &app);
    println!("\n-- SIMULATE watcher refresh mid-drag (e.g. nvim swap file write) --");
    app.refresh_from_disk();
    report("after refresh", &app);
    run(&mut app, vec![Event::PointerMoved(p2)]);
    report("drag after refresh", &app);
    run(&mut app, vec![Event::PointerMoved(p2)]);
    report("drag2", &app);
    run(
        &mut app,
        vec![Event::PointerButton {
            pos: p2,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    report("release", &app);
    run(&mut app, vec![]);
    report("idle after rel", &app);

    println!("\n-- mouse: click the Stage lines button @ (690,47) --");
    let b = egui::pos2(690.0, 47.0);
    run(&mut app, vec![Event::PointerMoved(b)]);
    report("btn move", &app);
    run(
        &mut app,
        vec![Event::PointerButton {
            pos: b,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    report("btn press", &app);
    run(
        &mut app,
        vec![Event::PointerButton {
            pos: b,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    report("after btn click", &app);
    let staged = repo::load_status(path).map(|(s, _)| s.len()).unwrap_or(0);
    println!("staged entries now = {staged}  (expect >0 if stage worked)");
}

fn dump(path: &std::path::Path) {
    match repo::discover(path) {
        Ok(node) => print_node(&node, 0),
        Err(e) => {
            eprintln!("discover failed: {e}");
            return;
        }
    }
    println!("\n--- selected repo data ({}) ---", path.display());
    match repo::build_graph(path, 200) {
        Ok(g) => {
            println!("commits: {} (max_col={})", g.rows.len(), g.max_col);
            for r in &g.rows {
                let segs: Vec<String> = r
                    .segments
                    .iter()
                    .map(|s| match s {
                        repo::Segment::Through { col, .. } => format!("through{col}"),
                        repo::Segment::TopToNode { col, .. } => format!("top{col}→node"),
                        repo::Segment::NodeToBottom { col, .. } => format!("node→bot{col}"),
                    })
                    .collect();
                let head = if r.is_head { "* " } else { "  " };
                let refs = if r.refs.is_empty() {
                    String::new()
                } else {
                    let names: Vec<String> = r
                        .refs
                        .iter()
                        .map(|rf| {
                            if rf.is_head {
                                format!("HEAD->{}", rf.name)
                            } else {
                                rf.name.clone()
                            }
                        })
                        .collect();
                    format!(" ({})", names.join(", "))
                };
                println!(
                    "{head}col{} {} {:<18}{} [{}]",
                    r.node_col,
                    r.short_id,
                    r.summary,
                    refs,
                    segs.join(", ")
                );
            }
        }
        Err(e) => eprintln!("graph failed: {e}"),
    }
    match repo::load_status(path) {
        Ok((staged, unstaged)) => {
            println!("staged: {}", staged.len());
            for e in &staged {
                println!("  {} {}", e.kind.marker(), e.path);
            }
            println!("unstaged: {}", unstaged.len());
            for e in &unstaged {
                println!("  {} {}", e.kind.marker(), e.path);
            }
        }
        Err(e) => eprintln!("status failed: {e}"),
    }
}

fn print_diff(d: &repo::FileDiff) {
    if let Some(note) = &d.note {
        println!("{note}");
        return;
    }
    for (i, row) in d.rows.iter().enumerate() {
        match row {
            repo::DiffRow::FileHeader(p) => println!("r{i:<3} === {p} ==="),
            repo::DiffRow::Hunk { index, header } => println!("r{i:<3} [h{index}] {header}"),
            repo::DiffRow::Line {
                old_no,
                new_no,
                left,
                right,
                kind,
            } => {
                let mark = match kind {
                    repo::LineKind::Context => " ",
                    repo::LineKind::Added => "+",
                    repo::LineKind::Removed => "-",
                    repo::LineKind::Changed => "~",
                };
                let o = old_no.map(|n| n.to_string()).unwrap_or_default();
                let n = new_no.map(|n| n.to_string()).unwrap_or_default();
                println!(
                    "r{i:<3}{mark} {o:>3}|{:<20} {n:>3}|{}",
                    left.as_deref().unwrap_or(""),
                    right.as_deref().unwrap_or("")
                );
            }
        }
    }
}

fn print_node(node: &repo::RepoNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let tag = if node.initialized {
        ""
    } else {
        " (uninitialized)"
    };
    println!("{indent}{}{tag}  [{}]", node.name, node.path.display());
    for child in &node.children {
        print_node(child, depth + 1);
    }
}
