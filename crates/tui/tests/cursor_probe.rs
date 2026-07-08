use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use twit::app::{Pane, Tab, TuiApp};
use twit::term::CursorStyle;
use twit::ui;

fn isolate_xdg() {
    static INIT: std::sync::Once = std::sync::Once::new();
    let dir = std::env::temp_dir().join(format!("twit-cursor-xdg-{}", std::process::id()));
    INIT.call_once(|| {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
            std::env::set_var("XDG_DATA_HOME", dir.join("data"));
            std::env::set_var("XDG_STATE_HOME", dir.join("state"));
        }
    });
}

fn has_nvim() -> bool {
    Command::new("nvim").arg("--version").output().is_ok()
}

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
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("twit-cursor-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["config", "user.email", "t@t"]);
    dir
}

fn render(app: &mut TuiApp) {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
}

fn pump_settle(app: &mut TuiApp, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        if let Some(t) = app.term.as_mut() {
            t.pump();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn editor_cursor_shape_tracks_nvim_mode() {
    if !has_nvim() {
        eprintln!("nvim missing; skipping");
        return;
    }
    isolate_xdg();
    let dir = temp_repo();
    std::fs::write(dir.join("hello.txt"), "one\ntwo\nthree\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    let file = dir.join("hello.txt");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(app.open_in_embedded(&file, None));
        app.poll_pending_open();
        pump_settle(&mut app, 100);
        render(&mut app);
        if app.editor_cursor_shape.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "editor never showed a cursor");
    }
    assert_eq!(app.active_tab, Tab::Editor);

    // Normal mode → block
    pump_settle(&mut app, 200);
    render(&mut app);
    assert_eq!(
        app.editor_cursor_shape,
        Some(CursorStyle::Block),
        "normal mode is a block cursor"
    );

    // Insert mode → bar
    app.handle_input(vec![key(KeyCode::Char('i'))]);
    pump_settle(&mut app, 400);
    render(&mut app);
    assert_eq!(
        app.editor_cursor_shape,
        Some(CursorStyle::Bar),
        "insert mode is a bar cursor"
    );

    // Back to normal → block
    app.handle_input(vec![key(KeyCode::Esc)]);
    pump_settle(&mut app, 400);
    render(&mut app);
    assert_eq!(
        app.editor_cursor_shape,
        Some(CursorStyle::Block),
        "escape returns to a block cursor"
    );

    let socket = app.nvim_socket.to_string_lossy().into_owned();
    drop(app);
    let _ = Command::new("pkill").args(["-f", &socket]).status();
    let _ = std::fs::remove_dir_all(&dir);
}
