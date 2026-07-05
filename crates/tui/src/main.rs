use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent};
use twig_core::watch::WorktreeWatcher;

use twig_tui::app::{TuiApp, View, ViewMode};
use twig_tui::session::{self, Session};
use twig_tui::{clipboard, ui, zellij};

struct Args {
    repo: String,
    view: Option<View>,
    session: Option<String>,
    single: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        repo: ".".to_string(),
        view: None,
        session: None,
        single: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--view" => {
                let v = it.next().ok_or("--view requires a value")?;
                args.view =
                    Some(View::parse(&v).ok_or_else(|| format!("unknown view: {v}"))?);
            }
            "--session" => {
                args.session = Some(it.next().ok_or("--session requires a value")?);
            }
            "--single" => args.single = true,
            _ => args.repo = a,
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("twig-tui: {e}");
            std::process::exit(2);
        }
    };
    let path = match PathBuf::from(&args.repo).canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("twig-tui: cannot open {}: {e}", args.repo);
            std::process::exit(1);
        }
    };

    if args.view.is_none() && !args.single && zellij::inside_zellij() {
        match zellij::spawn_tab(&path) {
            Ok(()) => return,
            Err(e) => eprintln!("twig-tui: zellij split failed ({e}); running single window"),
        }
    }

    let view_mode = args.view.map(ViewMode::Single).unwrap_or(ViewMode::All);
    let mut app = match TuiApp::with_view(&path, view_mode) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("twig-tui: {e}");
            std::process::exit(1);
        }
    };
    if let ViewMode::Single(view) = view_mode {
        let token = args.session.unwrap_or_else(|| session::repo_token(&path));
        let dir = session::session_dir(&token);
        let zellij_pane = std::env::var("ZELLIJ_PANE_ID").ok().filter(|v| !v.is_empty());
        match Session::join(&dir, view.name(), std::process::id(), &path, zellij_pane) {
            Ok(s) => app.session = Some(s),
            Err(e) => app.error = Some(format!("session: {e}")),
        }
    }

    let (watch_tx, watch_rx) = mpsc::channel::<()>();
    let watcher = WorktreeWatcher::new(
        &path,
        Arc::new(move || {
            let _ = watch_tx.send(());
        }),
    );
    if let Err(e) = &watcher {
        app.error = Some(format!("watcher: {e}"));
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &watch_rx, watcher.ok().as_ref());
    ratatui::restore();
    let broadcast = app.quit_broadcast;
    if let Some(mut sess) = app.session.take() {
        sess.shutdown(broadcast);
    }
    if let Err(e) = result {
        eprintln!("twig-tui: {e}");
        std::process::exit(1);
    }
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut TuiApp,
    watch_rx: &mpsc::Receiver<()>,
    watcher: Option<&WorktreeWatcher>,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|frame| ui::draw(frame, app))?;

    loop {
        let mut keys: Vec<KeyEvent> = Vec::new();
        let mut dirty = false;

        if event::poll(Duration::from_millis(250))? {
            loop {
                match event::read()? {
                    Event::Key(k) => keys.push(k),
                    Event::Resize(_, _) => dirty = true,
                    _ => {}
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }

        let mut refresh = false;
        while watch_rx.try_recv().is_ok() {
            refresh = true;
        }
        if refresh && watcher.is_none_or(|w| w.take_dirty()) {
            app.refresh();
            dirty = true;
        }

        if app.sync_session() {
            dirty = true;
        }

        if !keys.is_empty() {
            app.handle_input(keys);
            dirty = true;
        }

        if app.pending_focus_jump {
            app.pending_focus_jump = false;
            if let Some(target) = app.session.as_ref().and_then(|s| s.diff_target_pane()) {
                std::thread::spawn(move || zellij::focus_pane(&target));
            }
        }
        if let Some(text) = app.pending_copy.take() {
            clipboard::copy(&text)?;
        }
        if let Some(file) = app.pending_editor.take() {
            open_editor(terminal, app, &file)?;
            dirty = true;
        }
        if app.quit {
            return Ok(());
        }
        if dirty {
            terminal.draw(|frame| ui::draw(frame, app))?;
        }
    }
}

fn open_editor(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut TuiApp,
    file: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };

    if let Some(server) = nvim_server() {
        if let Err(e) = twig_core::editor::open_abs_in_server(file, std::path::Path::new(&server))
        {
            app.error = Some(e);
        }
        return Ok(());
    }

    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    let status = std::process::Command::new("nvim").arg(file).status();
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;
    match status {
        Ok(_) => app.refresh(),
        Err(e) => app.error = Some(format!("nvim failed: {e}")),
    }
    Ok(())
}

fn nvim_server() -> Option<String> {
    ["TWIG_NVIM_ADDRESS", "NVIM"]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
}
