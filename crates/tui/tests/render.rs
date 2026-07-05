use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use twig_tui::app::{Pane, Tab, TuiApp};
use twig_tui::ui;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git runs");
    assert!(status.status.success(), "git {args:?} failed");
}

fn temp_repo() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("twig-tui-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["config", "user.email", "t@t"]);
    dir
}

fn screen(app: &mut TuiApp, w: u16, h: u16) -> Vec<String> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..h)
        .map(|y| {
            let mut line = String::new();
            let mut skip = 0usize;
            for x in 0..w {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                if let Some(cell) = buffer.cell((x, y)) {
                    let sym = cell.symbol();
                    line.push_str(sym);
                    skip = unicode_width::UnicodeWidthStr::width(sym).saturating_sub(1);
                }
            }
            line.trim_end().to_string()
        })
        .collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn find_line<'a>(lines: &'a [String], needle: &str) -> Option<&'a String> {
    lines.iter().find(|l| l.contains(needle))
}

#[test]
fn changes_and_side_by_side_diff_render_with_cjk() {
    let dir = temp_repo();
    std::fs::write(dir.join("hello.rs"), "fn main() {\n    let x = 1;\n}\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(
        dir.join("hello.rs"),
        "fn main() {\n    let x = 2; // 日本語コメント\n}\n",
    )
    .unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "Changes (1)").is_some());
    assert!(find_line(&lines, "M hello.rs").is_some());

    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);
    assert_eq!(app.active_tab, Tab::Diff);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "@@").is_some(), "hunk header shown");
    let changed = find_line(&lines, "日本語コメント").expect("changed line rendered");
    assert!(
        changed.contains("let x = 1;"),
        "old content on the left of the same row: {changed}"
    );
    let no_line = find_line(&lines, "2 ").expect("line numbers");
    assert!(!no_line.is_empty());
}

#[test]
fn graph_renders_merge_topology_with_connectors() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    git(&dir, &["checkout", "-qb", "feature"]);
    std::fs::write(dir.join("f.txt"), "f\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "feature work"]);
    git(&dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("m.txt"), "m\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "main work"]);
    git(&dir, &["merge", "-q", "--no-ff", "feature", "-m", "merge feature"]);

    let mut app = TuiApp::new(&dir).unwrap();
    let lines = screen(&mut app, 140, 30);

    let merge = find_line(&lines, "merge feature").expect("merge row");
    assert!(merge.contains("●─╮"), "merge branches out: {merge}");
    let feature = find_line(&lines, "feature work").expect("feature row");
    assert!(feature.contains("│ ●"), "feature on lane 1: {feature}");
    let init = find_line(&lines, "init").expect("init row");
    assert!(init.contains("●─╯"), "feature lane merges back: {init}");

    let merge_idx = lines.iter().position(|l| l.contains("merge feature")).unwrap();
    let connector = &lines[merge_idx + 1];
    assert!(
        connector.trim_start().starts_with("│"),
        "connector row between commits: {connector:?}"
    );
}

#[test]
fn visual_select_and_yank_sets_pending_copy() {
    let dir = temp_repo();
    std::fs::write(dir.join("x.txt"), "one\ntwo\nthree\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("x.txt"), "one\nTWO\nthree\nfour\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);
    assert_eq!(app.active_tab, Tab::Diff);
    let _ = screen(&mut app, 120, 30);

    app.handle_input(vec![
        key(KeyCode::Char('v')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('y')),
    ]);
    let copied = app.pending_copy.take().expect("yank stored");
    assert!(copied.contains("TWO"), "copied selection: {copied:?}");
    assert!(app.diff_nav.anchor.is_none(), "visual cleared after yank");
}

#[test]
fn stage_via_space_and_commit_updates_graph() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![key(KeyCode::Char('j')), key(KeyCode::Char(' '))]);
    assert_eq!(app.staged.len(), 1, "space on the Changes group stages all files");

    app.handle_input(vec![key(KeyCode::Char('c'))]);
    assert!(app.prompt.is_some(), "c opens commit prompt");
    for ch in "test commit".chars() {
        app.handle_input(vec![key(KeyCode::Char(ch))]);
    }
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert!(app.prompt.is_none());
    assert!(app.staged.is_empty(), "staged consumed by commit");
    let mut found = false;
    let lines = screen(&mut app, 140, 30);
    for l in &lines {
        if l.contains("test commit") {
            found = true;
        }
    }
    assert!(found, "new commit appears in graph");
}

fn type_text(app: &mut TuiApp, text: &str) {
    for ch in text.chars() {
        app.handle_input(vec![key(KeyCode::Char(ch))]);
    }
}

#[test]
fn amend_prompt_prefills_head_message_and_rewrites_commit() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "first"]);
    std::fs::write(dir.join("a.txt"), "b\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char(' ')),
    ]);
    assert_eq!(app.staged.len(), 1);

    app.handle_input(vec![key(KeyCode::Char('a'))]);
    let (_, input) = app.prompt.as_ref().expect("a opens amend prompt");
    assert_eq!(input, "first", "prompt prefilled with head message");

    type_text(&mut app, " v2");
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert!(app.prompt.is_none(), "unpushed head amends without confirm");
    assert!(app.staged.is_empty(), "staged changes folded into head");
    let commits = app.graph.rows.iter().filter(|r| !r.is_uncommitted).count();
    assert_eq!(commits, 1, "amend must not add a commit");
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "first v2").is_some(), "graph shows amended message");
}

#[test]
fn discard_file_asks_confirmation_before_restoring() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('d')),
    ]);
    assert!(app.prompt.is_some(), "d asks for confirmation");
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "Discard changes to a.txt?").is_some());

    app.handle_input(vec![key(KeyCode::Char('n'))]);
    assert!(app.prompt.is_none());
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "two\n");

    app.handle_input(vec![key(KeyCode::Char('d')), key(KeyCode::Char('y'))]);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
    assert!(app.unstaged.is_empty(), "worktree clean after discard");
}

fn two_hunk_repo() -> PathBuf {
    let dir = temp_repo();
    let base: String = (1..=20).map(|i| format!("line{i}\n")).collect();
    std::fs::write(dir.join("x.txt"), &base).unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    let changed = base
        .replace("line2\n", "line2v2\n")
        .replace("line3\n", "line3v2\n")
        .replace("line18\n", "line18v2\n");
    std::fs::write(dir.join("x.txt"), changed).unwrap();
    dir
}

fn count_staged_changes(dir: &Path) -> usize {
    let d = twig_core::repo::file_diff(dir, "x.txt", twig_core::repo::DiffMode::Staged).unwrap();
    d.rows
        .iter()
        .filter(|r| {
            matches!(
                r,
                twig_core::repo::DiffRow::Line { kind, .. }
                    if *kind != twig_core::repo::LineKind::Context
            )
        })
        .count()
}

#[test]
fn visual_stage_selection_stages_only_selected_lines() {
    let dir = two_hunk_repo();
    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);
    assert_eq!(app.active_tab, Tab::Diff);

    app.handle_input(vec![key(KeyCode::Char('s'))]);
    assert_eq!(app.staged.len(), 1, "partially staged file appears in staged");
    assert_eq!(app.unstaged.len(), 1, "remaining lines stay unstaged");
    assert_eq!(count_staged_changes(&dir), 1, "only the cursor line staged");
}

#[test]
fn hunk_stage_stages_whole_hunk_under_cursor() {
    let dir = two_hunk_repo();
    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);

    app.handle_input(vec![KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)]);
    assert_eq!(app.staged.len(), 1);
    assert_eq!(app.unstaged.len(), 1, "second hunk still unstaged");
    assert_eq!(count_staged_changes(&dir), 2, "both lines of hunk 1 staged");
}

#[test]
fn diff_discard_selection_confirms_then_reverts_lines() {
    let dir = two_hunk_repo();
    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);

    app.handle_input(vec![key(KeyCode::Char('d'))]);
    assert!(app.prompt.is_some(), "line discard asks for confirmation");
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    let content = std::fs::read_to_string(dir.join("x.txt")).unwrap();
    assert!(content.contains("line2\n"), "cursor line reverted");
    assert!(content.contains("line3v2\n"), "other lines untouched");
    assert!(content.contains("line18v2\n"));
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn graph_create_branch_and_checkout_via_prompts() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;

    app.handle_input(vec![key(KeyCode::Char('b'))]);
    assert!(app.prompt.is_some(), "b opens branch name prompt");
    type_text(&mut app, "dev");
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert_eq!(git_out(&dir, &["branch", "--list", "dev"]), "dev");

    app.handle_input(vec![key(KeyCode::Char('o'))]);
    let lines = screen(&mut app, 140, 30);
    assert!(
        find_line(&lines, "Checkout: 1) branch dev").is_some(),
        "checkout prompt lists the non-head branch"
    );
    app.handle_input(vec![key(KeyCode::Char('1'))]);
    assert_eq!(git_out(&dir, &["symbolic-ref", "HEAD"]), "refs/heads/dev");
}

#[test]
fn graph_reset_soft_moves_head_and_keeps_index() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "first"]);
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "second"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    let target = app.graph.rows[1].id;

    app.handle_input(vec![
        key(KeyCode::Char('j')),
        KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT),
    ]);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "(s)oft / (m)ixed / (h)ard").is_some());

    app.handle_input(vec![key(KeyCode::Char('s'))]);
    assert_eq!(git_out(&dir, &["rev-parse", "HEAD"]), target.to_string());
    assert_eq!(app.staged.len(), 1, "soft reset keeps the change staged");
}

#[test]
fn graph_hard_reset_needs_second_confirmation() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "first"]);
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "second"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    let head = git_out(&dir, &["rev-parse", "HEAD"]);

    app.handle_input(vec![
        key(KeyCode::Char('j')),
        KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT),
        key(KeyCode::Char('h')),
    ]);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "Hard reset discards").is_some());
    app.handle_input(vec![key(KeyCode::Char('n'))]);
    assert_eq!(git_out(&dir, &["rev-parse", "HEAD"]), head, "n keeps HEAD");

    app.handle_input(vec![
        KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT),
        key(KeyCode::Char('h')),
        key(KeyCode::Char('y')),
    ]);
    assert_ne!(git_out(&dir, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
}

#[test]
fn graph_cherry_pick_applies_commit_onto_head() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    git(&dir, &["checkout", "-qb", "feat"]);
    std::fs::write(dir.join("f.txt"), "f\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "feat work"]);
    git(&dir, &["checkout", "-q", "main"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    let feat_row = app
        .graph
        .rows
        .iter()
        .position(|r| r.summary == "feat work")
        .expect("feat commit in graph");
    for _ in 0..feat_row {
        app.handle_input(vec![key(KeyCode::Char('j'))]);
    }

    app.handle_input(vec![key(KeyCode::Char('y'))]);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "Cherry-pick").is_some(), "confirm prompt");
    app.handle_input(vec![key(KeyCode::Char('y'))]);

    assert!(app.error.is_none(), "cherry-pick ok: {:?}", app.error);
    assert!(dir.join("f.txt").exists(), "picked file in worktree");
    assert_eq!(
        git_out(&dir, &["log", "--format=%s", "-1"]),
        "feat work",
        "picked commit on top of main"
    );
    assert_eq!(git_out(&dir, &["symbolic-ref", "HEAD"]), "refs/heads/main");
}

#[test]
fn graph_tag_create_delete_and_branch_rename() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;

    app.handle_input(vec![key(KeyCode::Char('t'))]);
    type_text(&mut app, "v1");
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert_eq!(git_out(&dir, &["tag", "-l", "v1"]), "v1");

    app.handle_input(vec![key(KeyCode::Char('x'))]);
    let lines = screen(&mut app, 140, 30);
    assert!(
        find_line(&lines, "Delete tag v1? (y/n)").is_some(),
        "single deletable ref confirms directly"
    );
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    assert_eq!(git_out(&dir, &["tag", "-l", "v1"]), "");

    app.handle_input(vec![key(KeyCode::Char('r'))]);
    let (_, input) = app.prompt.as_ref().expect("rename prompt");
    assert_eq!(input, "main", "prefilled with branch name");
    for _ in 0..4 {
        app.handle_input(vec![key(KeyCode::Backspace)]);
    }
    type_text(&mut app, "trunk");
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert_eq!(git_out(&dir, &["symbolic-ref", "HEAD"]), "refs/heads/trunk");
}

#[test]
fn graph_interactive_rebase_requests_suspended_git() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "first"]);
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "second"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    let head = app.graph.rows[0].id;

    app.handle_input(vec![key(KeyCode::Char('i'))]);
    let argv = app.pending_shell.take().expect("i schedules git rebase -i");
    assert_eq!(argv[0], "git");
    assert_eq!(argv[3..5], ["rebase".to_string(), "-i".to_string()]);
    assert_eq!(argv[5], format!("{head}^"));
}

fn wait_remote(app: &mut TuiApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.remote.is_some() {
        app.poll_remote();
        assert!(
            std::time::Instant::now() < deadline,
            "remote operation timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn changes_folder_row_bulk_stages_and_folds() {
    let dir = temp_repo();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.rs"), "a\n").unwrap();
    std::fs::write(dir.join("src/b.rs"), "b\n").unwrap();
    std::fs::write(dir.join("top.txt"), "t\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("src/a.rs"), "a2\n").unwrap();
    std::fs::write(dir.join("src/b.rs"), "b2\n").unwrap();
    std::fs::write(dir.join("top.txt"), "t2\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "▾ src/").is_some(), "folder row rendered");

    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('h')),
    ]);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "▸ src/").is_some(), "h folds the folder");
    assert!(find_line(&lines, "a.rs").is_none(), "children hidden while folded");
    assert!(find_line(&lines, "top.txt").is_some(), "siblings still listed");

    app.handle_input(vec![key(KeyCode::Char('l'))]);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "▾ src/").is_some(), "l unfolds the folder");
    assert!(find_line(&lines, "a.rs").is_some());

    app.handle_input(vec![key(KeyCode::Char(' '))]);
    assert_eq!(app.staged.len(), 2, "space on the folder stages its files");
    assert_eq!(app.unstaged.len(), 1, "sibling file stays unstaged");

    app.handle_input(vec![key(KeyCode::Char('j')), key(KeyCode::Char('h'))]);
    let items = app.changes_items();
    assert!(
        matches!(
            items[app.changes_cursor],
            twig_tui::app::ChangesItem::Folder { .. }
        ),
        "h on a child jumps to the parent folder"
    );

    app.handle_input(vec![key(KeyCode::Char(' '))]);
    assert!(app.staged.is_empty(), "space on the staged folder unstages");
    assert_eq!(app.unstaged.len(), 3);
}

#[test]
fn changes_group_row_bulk_discard_confirms() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    std::fs::write(dir.join("b.txt"), "two\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "one2\n").unwrap();
    std::fs::write(dir.join("b.txt"), "two2\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![key(KeyCode::Char('j')), key(KeyCode::Char('d'))]);
    let lines = screen(&mut app, 140, 30);
    assert!(
        find_line(&lines, "Discard changes to all 2 files? (y/n)").is_some(),
        "group discard confirms with a count"
    );
    app.handle_input(vec![key(KeyCode::Char('n'))]);
    assert_eq!(app.unstaged.len(), 2, "n keeps the changes");

    app.handle_input(vec![key(KeyCode::Char('d')), key(KeyCode::Char('y'))]);
    assert!(app.unstaged.is_empty(), "y discards every unstaged file");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
    assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "two\n");
}

#[test]
fn stash_push_show_and_pop_via_prompts() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![key(KeyCode::Char('z'))]);
    assert_eq!(app.stashes.len(), 1, "z stashes the worktree");
    assert!(app.unstaged.is_empty());
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "Stashes (1)").is_some());

    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);
    let lines = screen(&mut app, 140, 30);
    let row = find_line(&lines, "two").expect("stash diff rendered");
    assert!(row.contains("one"), "old content on the left: {row}");

    app.focus = Pane::Changes;
    app.handle_input(vec![key(KeyCode::Char(' '))]);
    let lines = screen(&mut app, 140, 30);
    assert!(
        find_line(&lines, "(p)op / (a)pply / (d)rop").is_some(),
        "space on a stash opens the op picker"
    );
    app.handle_input(vec![key(KeyCode::Char('p'))]);
    assert!(app.stashes.is_empty(), "pop removes the stash");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "two\n");
}

#[test]
fn stash_drop_requires_confirmation() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![key(KeyCode::Char('z'))]);
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char(' ')),
        key(KeyCode::Char('d')),
    ]);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "Drop stash@{0}? (y/n)").is_some());
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    assert!(app.stashes.is_empty());
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
}

#[test]
fn push_and_force_push_to_local_remote() {
    let dir = temp_repo();
    let origin = std::env::temp_dir().join(format!(
        "twig-tui-origin-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&origin);
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);

    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    git(&dir, &["remote", "add", "origin", origin.to_str().unwrap()]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;

    app.handle_input(vec![key(KeyCode::Char('p'))]);
    assert!(app.remote.is_some(), "p starts a push job");
    wait_remote(&mut app);
    assert!(app.error.is_none(), "push ok: {:?}", app.error);
    assert_eq!(
        git_out(&origin, &["rev-parse", "main"]),
        git_out(&dir, &["rev-parse", "HEAD"])
    );

    git(&dir, &["commit", "--amend", "-qm", "rewritten"]);
    app.refresh();
    app.handle_input(vec![KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT)]);
    let lines = screen(&mut app, 140, 30);
    assert!(
        find_line(&lines, "Force push current branch to origin? (y/n)").is_some(),
        "force push asks first"
    );
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    wait_remote(&mut app);
    assert!(app.error.is_none(), "force push ok: {:?}", app.error);
    assert_eq!(
        git_out(&origin, &["rev-parse", "main"]),
        git_out(&dir, &["rev-parse", "HEAD"])
    );
    let _ = std::fs::remove_dir_all(&origin);
}

#[test]
fn conflicted_cherry_pick_shows_banner_and_aborts() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "base\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "base"]);
    git(&dir, &["checkout", "-qb", "feat"]);
    std::fs::write(dir.join("a.txt"), "feat\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "feat change"]);
    git(&dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("a.txt"), "main\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "main change"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    let feat_row = app
        .graph
        .rows
        .iter()
        .position(|r| r.summary == "feat change")
        .expect("feat commit");
    for _ in 0..feat_row {
        app.handle_input(vec![key(KeyCode::Char('j'))]);
    }
    app.handle_input(vec![key(KeyCode::Char('y')), key(KeyCode::Char('y'))]);

    assert!(
        matches!(app.seq, Some((twig_core::repo::SeqState::CherryPick, _))),
        "conflict leaves the sequencer active"
    );
    let lines = screen(&mut app, 140, 30);
    let banner = find_line(&lines, "Cherry-pick in progress").expect("banner shown");
    assert!(banner.contains("a.txt"), "conflict listed: {banner}");

    app.handle_input(vec![KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)]);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "Abort the in-progress operation? (y/n)").is_some());
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    assert!(app.seq.is_none(), "abort clears the sequencer");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "main\n");
}

#[test]
fn search_tab_finds_and_replaces_across_repo() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "hello needle world\n").unwrap();
    std::fs::write(dir.join("b.txt"), "no match here\nneedle again\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    app.active_tab = Tab::Search;

    app.handle_input(vec![key(KeyCode::Char('/'))]);
    assert!(app.prompt.is_some(), "/ opens the search prompt");
    type_text(&mut app, "needle");
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert_eq!(app.search.hits.len(), 2, "both files hit");
    assert_eq!(app.search.match_count(), 2);
    let lines = screen(&mut app, 140, 30);
    assert!(find_line(&lines, "2 matches in 2 files").is_some());
    assert!(find_line(&lines, "needle again").is_some());

    app.handle_input(vec![key(KeyCode::Char('r'))]);
    type_text(&mut app, "thread");
    app.handle_input(vec![key(KeyCode::Enter)]);
    let lines = screen(&mut app, 140, 30);
    assert!(
        find_line(&lines, "Replace all matches with \"thread\"? (y/n)").is_some(),
        "replace asks for confirmation"
    );
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "hello thread world\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("b.txt")).unwrap(),
        "no match here\nthread again\n"
    );
    assert!(app.search.hits.is_empty(), "results refreshed after replace");
}

#[test]
fn diff_find_jumps_and_highlights() {
    let dir = two_hunk_repo();
    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);
    assert_eq!(app.active_tab, Tab::Diff);

    app.handle_input(vec![key(KeyCode::Char('/'))]);
    assert!(app.prompt.is_some(), "/ opens find prompt in diff");
    type_text(&mut app, "line18v2");
    app.handle_input(vec![key(KeyCode::Enter)]);
    let row = &app.diff.rows[app.diff_nav.cursor];
    let hit = match row {
        twig_core::repo::DiffRow::Line { right, .. } => {
            right.as_deref().unwrap_or("").contains("line18v2")
        }
        _ => false,
    };
    assert!(hit, "cursor jumped to the match");

    let before = app.diff_nav.cursor;
    app.handle_input(vec![key(KeyCode::Char('n'))]);
    assert_eq!(app.diff_nav.cursor, before, "single match wraps to itself");

    app.handle_input(vec![key(KeyCode::Esc)]);
    assert!(app.diff_find.is_none(), "esc clears the find query");
}

#[test]
fn help_overlay_toggles_without_quitting() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![key(KeyCode::Char('?'))]);
    assert!(app.help_open);
    let lines = screen(&mut app, 140, 40);
    assert!(find_line(&lines, "Keybindings").is_some());
    assert!(find_line(&lines, "Stage/unstage the item under the cursor").is_some());

    app.handle_input(vec![key(KeyCode::Char('q'))]);
    assert!(!app.help_open, "q closes the help overlay");
    assert!(!app.quit, "q inside help must not quit the app");
}

#[test]
fn sidebar_initializes_submodule_via_prompt() {
    let child = temp_repo();
    std::fs::write(child.join("c.txt"), "c\n").unwrap();
    git(&child, &["add", "-A"]);
    git(&child, &["commit", "-qm", "child init"]);

    let parent = temp_repo();
    std::fs::write(parent.join("p.txt"), "p\n").unwrap();
    git(&parent, &["add", "-A"]);
    git(&parent, &["commit", "-qm", "parent init"]);
    git(
        &parent,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            child.to_str().unwrap(),
            "sub",
        ],
    );
    git(&parent, &["commit", "-qm", "add submodule"]);

    let clone = std::env::temp_dir().join(format!(
        "twig-tui-clone-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&clone);
    git(
        &parent,
        &["clone", "-q", parent.to_str().unwrap(), clone.to_str().unwrap()],
    );

    let mut app = TuiApp::new(&clone).unwrap();
    app.focus = Pane::Sidebar;
    let rows = app.sidebar_rows();
    assert_eq!(rows.len(), 2, "clone shows the submodule row");
    assert!(!rows[1].initialized, "submodule starts uninitialized");

    app.handle_input(vec![key(KeyCode::Char('j')), key(KeyCode::Char('i'))]);
    let lines = screen(&mut app, 140, 30);
    assert!(
        find_line(&lines, "Initialize submodule sub (clone)? (y/n)").is_some(),
        "init asks for confirmation"
    );
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    wait_remote(&mut app);
    assert!(app.error.is_none(), "init ok: {:?}", app.error);
    let rows = app.sidebar_rows();
    assert!(rows[1].initialized, "rediscover marks the submodule ready");
    assert!(clone.join("sub/c.txt").exists(), "submodule cloned");
    let _ = std::fs::remove_dir_all(&clone);
}

#[test]
fn graph_expands_commit_files_and_opens_per_file_diff() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "first\n").unwrap();
    std::fs::write(dir.join("b.txt"), "bee\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "second\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "change a"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert!(app.selected_commit.is_some(), "enter selects head commit");
    assert_eq!(app.commit_files.len(), 1);
    assert_eq!(
        app.active_tab,
        Tab::Graph,
        "stays on graph to pick a file; whole-commit diff loads in background"
    );
    assert!(!app.diff.rows.is_empty(), "whole-commit diff loaded");

    let lines = screen(&mut app, 140, 30);
    let commit_idx = lines
        .iter()
        .position(|l| l.contains("change a"))
        .expect("commit row");
    assert!(
        lines[commit_idx + 1].contains("M a.txt"),
        "file row right under expanded commit: {:?}",
        &lines[commit_idx..commit_idx + 2]
    );

    app.handle_input(vec![key(KeyCode::Char('j')), key(KeyCode::Enter)]);
    assert_eq!(app.selected_commit_file, Some("a.txt".to_string()));
    assert_eq!(app.active_tab, Tab::Diff);
    let lines = screen(&mut app, 140, 30);
    let row = find_line(&lines, "second").expect("per-file diff rendered");
    assert!(row.contains("first"), "old content on the left: {row}");

    app.active_tab = Tab::Graph;
    app.handle_input(vec![key(KeyCode::Char('h'))]);
    let items = app.graph_items();
    assert!(
        matches!(items[app.graph_cursor], twig_tui::app::GraphItem::Commit(_)),
        "h jumps back to the commit row"
    );
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert_eq!(app.selected_commit_file, None, "enter reopens whole-commit diff");
    assert_eq!(app.active_tab, Tab::Graph, "still on graph after reopening");
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert!(app.selected_commit.is_none(), "second enter collapses");
    assert!(app.commit_files.is_empty());
}
