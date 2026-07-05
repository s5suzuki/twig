use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use twig_tui::app::{Pane, Tab, TuiApp};
use twig_tui::ui;

fn isolate_xdg() -> PathBuf {
    static INIT: std::sync::Once = std::sync::Once::new();
    let dir = std::env::temp_dir().join(format!("twig-tui-editor-xdg-{}", std::process::id()));
    INIT.call_once(|| {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
            std::env::set_var("XDG_DATA_HOME", dir.join("data"));
            std::env::set_var("XDG_STATE_HOME", dir.join("state"));
        }
    });
    dir
}

fn has_nvim() -> bool {
    Command::new("nvim").arg("--version").output().is_ok()
}

fn cleanup(app: TuiApp, dirs: &[&Path]) {
    let socket = app.nvim_socket.to_string_lossy().into_owned();
    drop(app);
    let _ = Command::new("pkill").args(["-f", &socket]).status();
    for d in dirs {
        let _ = std::fs::remove_dir_all(d);
    }
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
    let dir = std::env::temp_dir().join(format!("twig-tui-editor-{}-{n}", std::process::id()));
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

fn wait_for(app: &mut TuiApp, needle: &str, secs: u64) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(t) = app.term.as_mut() {
            t.pump();
        }
        let lines = screen(app, 120, 35);
        if lines.iter().any(|l| l.contains(needle)) {
            return lines;
        }
        assert!(
            Instant::now() < deadline,
            "never saw {needle:?} on screen:\n{}",
            lines.join("\n")
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn embedded_nvim_tab_starts_edits_and_keeps_q_local() {
    if !has_nvim() {
        return;
    }
    isolate_xdg();
    let dir = temp_repo();
    std::fs::write(dir.join("hello.txt"), "embedded-editor-line 日本語行\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);

    let mut app = TuiApp::new(&dir).unwrap();
    app.focus = Pane::RightTab;
    app.handle_input(vec![key(KeyCode::Tab), key(KeyCode::Tab), key(KeyCode::Tab)]);
    assert_eq!(app.active_tab, Tab::Editor, "tab cycles into the editor tab");
    assert!(app.term.is_some(), "entering the tab spawns nvim");

    let file = dir.join("hello.txt");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(app.open_in_embedded(&file), "embedded editor accepts the file");
        if let Some(t) = app.term.as_mut() {
            t.pump();
        }
        std::thread::sleep(Duration::from_millis(100));
        let lines = screen(&mut app, 120, 35);
        if lines.iter().any(|l| l.contains("embedded-editor-line")) {
            assert!(
                lines.iter().any(|l| l.contains("日本語行")),
                "CJK renders in the grid:\n{}",
                lines.join("\n")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "file never appeared:\n{}",
            lines.join("\n")
        );
    }

    app.handle_input(vec![key(KeyCode::Char('q'))]);
    assert!(!app.quit, "q goes to nvim while the editor is focused");
    app.handle_input(vec![key(KeyCode::Esc)]);
    std::thread::sleep(Duration::from_millis(200));

    app.handle_input(vec![
        key(KeyCode::Char('g')),
        key(KeyCode::Char('g')),
        key(KeyCode::Char('O')),
        key(KeyCode::Char('t')),
        key(KeyCode::Char('y')),
        key(KeyCode::Char('p')),
        key(KeyCode::Char('e')),
        key(KeyCode::Char('d')),
        key(KeyCode::Esc),
    ]);
    wait_for(&mut app, "typed", 10);

    app.handle_input(vec![key(KeyCode::Tab)]);
    assert_eq!(
        app.active_tab,
        Tab::Graph,
        "tab cycles out of the editor like the GUI"
    );
    app.handle_input(vec![key(KeyCode::BackTab)]);
    assert_eq!(app.active_tab, Tab::Editor, "shift+tab cycles back in");

    let mut ev = key(KeyCode::Char('h'));
    ev.modifiers = KeyModifiers::ALT;
    app.handle_input(vec![ev]);
    assert_ne!(app.focus, Pane::RightTab, "alt+h leaves the editor pane");

    cleanup(app, &[&dir]);
}

#[test]
fn open_in_embedded_spawns_nvim_and_opens_the_file() {
    if !has_nvim() {
        return;
    }
    isolate_xdg();
    let dir = temp_repo();
    std::fs::write(dir.join("from-e.txt"), "opened-via-e-key\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);

    let mut app = TuiApp::new(&dir).unwrap();
    assert!(app.term.is_none());

    let file = dir.join("from-e.txt");
    assert!(
        app.open_in_embedded(&file),
        "e path spawns the editor even when it is not running yet"
    );
    assert_eq!(app.active_tab, Tab::Editor, "switches to the editor tab");
    assert!(app.term.is_some(), "nvim spawned on demand");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_pending_open();
        if let Some(t) = app.term.as_mut() {
            t.pump();
        }
        let lines = screen(&mut app, 120, 35);
        if lines.iter().any(|l| l.contains("opened-via-e-key")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "file never opened:\n{}",
            lines.join("\n")
        );
        std::thread::sleep(Duration::from_millis(30));
    }

    cleanup(app, &[&dir]);
}
