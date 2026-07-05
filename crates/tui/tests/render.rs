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

    app.handle_input(vec![key(KeyCode::Enter)]);
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
    app.handle_input(vec![key(KeyCode::Enter)]);
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
    app.handle_input(vec![key(KeyCode::Char(' '))]);
    assert_eq!(app.staged.len(), 1, "space stages the file");

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
    app.handle_input(vec![key(KeyCode::Char(' '))]);
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
    app.handle_input(vec![key(KeyCode::Char('d'))]);
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
    app.handle_input(vec![key(KeyCode::Enter)]);
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
    app.handle_input(vec![key(KeyCode::Enter)]);

    app.handle_input(vec![KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)]);
    assert_eq!(app.staged.len(), 1);
    assert_eq!(app.unstaged.len(), 1, "second hunk still unstaged");
    assert_eq!(count_staged_changes(&dir), 2, "both lines of hunk 1 staged");
}

#[test]
fn diff_discard_selection_confirms_then_reverts_lines() {
    let dir = two_hunk_repo();
    let mut app = TuiApp::new(&dir).unwrap();
    app.handle_input(vec![key(KeyCode::Enter)]);

    app.handle_input(vec![key(KeyCode::Char('d'))]);
    assert!(app.prompt.is_some(), "line discard asks for confirmation");
    app.handle_input(vec![key(KeyCode::Char('y'))]);
    let content = std::fs::read_to_string(dir.join("x.txt")).unwrap();
    assert!(content.contains("line2\n"), "cursor line reverted");
    assert!(content.contains("line3v2\n"), "other lines untouched");
    assert!(content.contains("line18v2\n"));
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
    assert_eq!(app.active_tab, Tab::Diff, "whole-commit diff opens");

    app.active_tab = Tab::Graph;
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
    app.active_tab = Tab::Graph;
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert!(app.selected_commit.is_none(), "second enter collapses");
    assert!(app.commit_files.is_empty());
}
