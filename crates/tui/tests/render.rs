use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use twig_tui::app::{Tab, TuiApp};
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
    assert!(app.commit_input.is_some(), "c opens commit prompt");
    for ch in "test commit".chars() {
        app.handle_input(vec![key(KeyCode::Char(ch))]);
    }
    app.handle_input(vec![key(KeyCode::Enter)]);
    assert!(app.commit_input.is_none());
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
