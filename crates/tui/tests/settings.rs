use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use twit::app::TuiApp;
use twit::ui;

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
    let dir = std::env::temp_dir().join(format!("twit-settings-{}", std::process::id()));
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
            (0..w)
                .filter_map(|x| buffer.cell((x, y)).map(|c| c.symbol()))
                .collect::<String>()
                .trim_end()
                .to_string()
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
fn settings_overlay_edits_save_and_apply_immediately() {
    let conf = std::env::temp_dir().join(format!("twit-conf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&conf);
    std::fs::create_dir_all(&conf).unwrap();
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &conf) };

    let dir = temp_repo();
    for i in 0..3 {
        std::fs::write(dir.join("a.txt"), format!("v{i}\n")).unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-qm", &format!("c{i}")]);
    }

    let mut app = TuiApp::new(&dir).unwrap();
    let rows_before = app.graph.rows.len();
    assert_eq!(rows_before, 3);

    app.handle_input(vec![key(KeyCode::Char(','))]);
    assert!(app.settings_open, ", opens the settings overlay");
    let lines = screen(&mut app, 100, 20);
    assert!(find_line(&lines, "graph_commit_limit").is_some());
    assert!(find_line(&lines, "confirm_discard").is_some());

    app.handle_input(vec![key(KeyCode::Enter)]);
    let (_, input) = app.prompt.as_ref().expect("enter edits the number");
    assert_eq!(input, "200", "prefilled with the current value");
    for _ in 0..3 {
        app.handle_input(vec![key(KeyCode::Backspace)]);
    }
    app.handle_input(vec![key(KeyCode::Char('1')), key(KeyCode::Enter)]);
    assert_eq!(app.config.graph_commit_limit, 1);
    assert_eq!(app.graph.rows.len(), 1, "graph reloaded with the new limit");

    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Enter),
    ]);
    assert!(!app.config.confirm_discard, "enter toggles the bool");

    let text = std::fs::read_to_string(conf.join("twig/config.toml")).unwrap();
    assert!(text.contains("graph_commit_limit = 1"), "saved: {text}");
    assert!(text.contains("confirm_discard = false"), "saved: {text}");
    let reloaded = twit_core::config::Config::load();
    assert_eq!(reloaded.graph_commit_limit, 1);
    assert!(!reloaded.confirm_discard);

    app.handle_input(vec![key(KeyCode::Char(','))]);
    assert!(!app.settings_open, ", closes the overlay");

    std::fs::write(dir.join("a.txt"), "dirty\n").unwrap();
    app.refresh();
    app.handle_input(vec![
        key(KeyCode::Char('j')),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('d')),
    ]);
    assert!(
        app.prompt.is_none(),
        "confirm_discard=false discards without asking"
    );
    assert!(app.unstaged.is_empty(), "file restored");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "v2\n");

    let _ = std::fs::remove_dir_all(&conf);
    let _ = std::fs::remove_dir_all(&dir);
}
