use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent};
use twig_core::watch::WorktreeWatcher;

use twig_tui::app::{Tab, TuiApp, View, ViewMode};
use twig_tui::session::{self, Session};
use twig_tui::{clipboard, ui, zellij};

struct Args {
    repo: String,
    view: Option<View>,
    session: Option<String>,
    single: bool,
    new_tab: bool,
    shell: bool,
    cols: Option<u16>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        repo: ".".to_string(),
        view: None,
        session: None,
        single: false,
        new_tab: false,
        shell: false,
        cols: None,
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
            "--new-tab" => args.new_tab = true,
            "--shell" => args.shell = true,
            "--cols" => {
                let v = it.next().ok_or("--cols requires a value")?;
                args.cols = Some(v.parse().map_err(|_| format!("invalid --cols: {v}"))?);
            }
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

    if args.shell {
        run_shell(&path, args.session);
    }

    let mut view = args.view;
    let mut session_token = args.session;
    let mut cols = args.cols;
    if view.is_none() && !args.single && zellij::inside_zellij() {
        if args.new_tab {
            match zellij::spawn_tab(&path) {
                Ok(()) => return,
                Err(e) => eprintln!("twig-tui: zellij split failed ({e}); running single window"),
            }
        } else {
            let token = session::pid_token();
            match zellij::split_current_tab(&path, &token) {
                Ok(()) => {
                    view = Some(View::Sidebar);
                    session_token = Some(token);
                    cols = Some(26);
                }
                Err(e) => {
                    eprintln!("twig-tui: zellij split failed ({e}); running single window")
                }
            }
        }
    }

    let view_mode = view.map(ViewMode::Single).unwrap_or(ViewMode::All);
    let mut app = match TuiApp::with_view(&path, view_mode) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("twig-tui: {e}");
            std::process::exit(1);
        }
    };
    if let ViewMode::Single(view) = view_mode {
        let token = session_token.unwrap_or_else(|| session::repo_token(&path));
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
    let result = run(&mut terminal, &mut app, &watch_rx, watcher.ok().as_ref(), cols);
    ratatui::restore();
    let broadcast = app.quit_broadcast;
    if let Some(mut sess) = app.session.take() {
        for id in sess.shutdown(broadcast) {
            zellij::close_pane(&id);
        }
    }
    if let Err(e) = result {
        eprintln!("twig-tui: {e}");
        std::process::exit(1);
    }
}

fn run_shell(repo: &std::path::Path, session_token: Option<String>) -> ! {
    let token = session_token.unwrap_or_else(|| session::repo_token(repo));
    let dir = session::session_dir(&token);
    if let Ok(id) = std::env::var("ZELLIJ_PANE_ID")
        && !id.is_empty()
    {
        let _ = session::register_extra_pane(&dir, repo, &id);
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let mut child = match std::process::Command::new(&shell).current_dir(repo).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("twig-tui: failed to start {shell}: {e}");
            std::process::exit(1);
        }
    };
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            std::process::exit(status.code().unwrap_or(0));
        }
        if session::state_gone_or_quit(&dir) {
            let _ = std::process::Command::new("kill")
                .args(["-HUP", &child.id().to_string()])
                .status();
            std::thread::sleep(Duration::from_millis(300));
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
            std::process::exit(0);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut TuiApp,
    watch_rx: &mpsc::Receiver<()>,
    watcher: Option<&WorktreeWatcher>,
    cols: Option<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|frame| ui::draw(frame, app))?;

    struct Shrink {
        target: u16,
        deadline: std::time::Instant,
        prev: Option<u16>,
        step: u16,
    }
    let mut shrink = cols.map(|target| Shrink {
        target,
        deadline: std::time::Instant::now() + Duration::from_secs(5),
        prev: None,
        step: 0,
    });

    loop {
        if let Some(s) = shrink.as_mut() {
            if std::time::Instant::now() >= s.deadline {
                shrink = None;
            } else if let Ok((w, _)) = ratatui::crossterm::terminal::size() {
                if let Some(p) = s.prev
                    && p > w
                {
                    s.step = p - w;
                }
                if w.saturating_sub(s.target) > s.step / 2 {
                    zellij::resize_self_step();
                    s.prev = Some(w);
                }
            }
        }

        let mut keys: Vec<KeyEvent> = Vec::new();
        let mut dirty = false;

        let editor_visible = app.active_tab == Tab::Editor && app.term.is_some();
        let timeout = if editor_visible {
            Duration::from_millis(15)
        } else {
            Duration::from_millis(250)
        };
        if event::poll(timeout)? {
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

        if app.poll_remote() {
            dirty = true;
        }

        if app.poll_diff_recheck() {
            dirty = true;
        }

        if let Some(term) = app.term.as_mut() {
            if term.pump() {
                dirty = true;
            }
            if !term.is_alive() {
                app.term = None;
                dirty = true;
            }
        }

        if app.poll_pending_open() {
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
        if let Some(argv) = app.pending_shell.take() {
            run_suspended(terminal, app, &argv)?;
            dirty = true;
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

fn run_suspended(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut TuiApp,
    argv: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };

    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    let status = std::process::Command::new(&argv[0]).args(&argv[1..]).status();
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;
    match status {
        Ok(_) => app.refresh(),
        Err(e) => app.error = Some(format!("{} failed: {e}", argv[0])),
    }
    Ok(())
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

    if app.open_in_embedded(file) {
        return Ok(());
    }

    if let Some(sess) = app.session.as_mut()
        && sess.request_editor(file)
    {
        if let Some(target) = sess.editor_target_pane() {
            std::thread::spawn(move || zellij::focus_pane(&target));
        }
        return Ok(());
    }

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
