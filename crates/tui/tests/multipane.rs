use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use twig_tui::app::{Pane, Tab, TuiApp, View, ViewMode};
use twig_tui::session::Session;
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

fn temp_dir(kind: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "twig-multipane-{kind}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn temp_repo() -> PathBuf {
    let dir = temp_dir("repo");
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

fn pane(repo: &Path, session_dir: &Path, view: View) -> TuiApp {
    let mut app = TuiApp::with_view(repo, ViewMode::Single(view)).unwrap();
    app.session = Some(
        Session::join(session_dir, view.name(), std::process::id(), repo, None).unwrap(),
    );
    app
}

#[test]
fn single_changes_view_renders_full_frame() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();

    let mut app = TuiApp::with_view(&dir, ViewMode::Single(View::Changes)).unwrap();
    let lines = screen(&mut app, 60, 20);
    assert!(lines.iter().any(|l| l.contains("Changes (1)")));
    assert!(lines.iter().any(|l| l.contains("M a.txt")));
    assert!(
        !lines.iter().any(|l| l.contains("Repositories")),
        "sidebar must not render in changes view"
    );
    assert!(
        !lines.iter().any(|l| l.contains("Graph")),
        "right pane must not render in changes view"
    );
    assert!(
        !lines
            .iter()
            .any(|l| l.contains(['┌', '┐', '└', '┘', '│', '─'])),
        "single view draws no borders (zellij frames the pane)"
    );

    let mut all = TuiApp::new(&dir).unwrap();
    let lines = screen(&mut all, 120, 20);
    assert!(
        lines.iter().any(|l| l.contains('┌')),
        "combined view keeps its own borders"
    );
}

#[test]
fn enter_in_changes_pane_shows_diff_in_main_pane() {
    let dir = temp_repo();
    std::fs::write(dir.join("x.txt"), "one\ntwo\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("x.txt"), "one\nTWO\n").unwrap();

    let sdir = temp_dir("session");
    let mut changes = pane(&dir, &sdir, View::Changes);
    let mut main = pane(&dir, &sdir, View::Main);
    main.sync_session();
    assert_eq!(main.active_tab, Tab::Graph);

    changes.handle_input(vec![key(KeyCode::Enter)]);
    assert_eq!(changes.focus, Pane::Changes, "focus stays in own pane");
    assert!(
        changes.pending_focus_jump,
        "opening a diff requests a zellij focus jump"
    );
    changes.pending_focus_jump = false;

    assert!(main.sync_session(), "main picks up the published selection");
    assert!(
        !main.pending_focus_jump,
        "applying a shared selection must not re-trigger a jump"
    );
    assert_eq!(main.active_tab, Tab::Diff);
    assert_eq!(main.selected_file, Some(("x.txt".to_string(), false)));
    assert!(!main.diff.rows.is_empty());
    let lines = screen(&mut main, 120, 30);
    assert!(lines.iter().any(|l| l.contains("@@")), "hunk header shown");
    assert!(lines.iter().any(|l| l.contains("TWO")));
}

#[test]
fn graph_pane_enter_shows_commit_diff_in_main_pane() {
    let dir = temp_repo();
    std::fs::write(dir.join("a.txt"), "first\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "second\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "change a"]);

    let sdir = temp_dir("session");
    let mut graph = pane(&dir, &sdir, View::Graph);
    let mut main = pane(&dir, &sdir, View::Main);
    main.sync_session();

    graph.handle_input(vec![key(KeyCode::Enter)]);
    assert!(graph.selected_commit.is_some());

    assert!(main.sync_session());
    assert_eq!(main.active_tab, Tab::Diff);
    assert_eq!(main.selected_commit, graph.selected_commit);
    assert!(!main.diff.rows.is_empty());
}

#[test]
fn sidebar_repo_switch_rescopes_other_panes() {
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

    let sdir = temp_dir("session");
    let mut sidebar = pane(&parent, &sdir, View::Sidebar);
    let mut main = pane(&parent, &sdir, View::Main);
    main.sync_session();

    sidebar.handle_input(vec![key(KeyCode::Char('j')), key(KeyCode::Enter)]);
    assert!(sidebar.selected.ends_with("sub"), "sidebar switched to submodule");

    assert!(main.sync_session());
    assert!(main.selected.ends_with("sub"), "main rescoped to submodule");
    assert!(main.graph.rows.iter().any(|r| r.summary.contains("child init")));
}

#[test]
fn quit_in_one_pane_stops_the_others() {
    let dir = temp_repo();
    let sdir = temp_dir("session");
    let mut a = pane(&dir, &sdir, View::Changes);
    let mut b = pane(&dir, &sdir, View::Main);
    b.sync_session();

    a.handle_input(vec![key(KeyCode::Char('q'))]);
    assert!(a.quit);
    assert!(a.quit_broadcast, "locally initiated quit broadcasts");
    let broadcast = a.quit_broadcast;
    a.session.take().unwrap().shutdown(broadcast);

    assert!(b.sync_session());
    assert!(b.quit, "sibling pane quits on next tick");
    assert!(!b.quit_broadcast, "received quit is not re-broadcast");
}
