mod app;
mod clipboard;
mod keys;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent};
use twig_core::watch::WorktreeWatcher;

use app::TuiApp;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let path = match PathBuf::from(&arg).canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("twig-tui: cannot open {arg}: {e}");
            std::process::exit(1);
        }
    };
    let mut app = match TuiApp::new(&path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("twig-tui: {e}");
            std::process::exit(1);
        }
    };

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

        if !keys.is_empty() {
            app.handle_input(keys);
            dirty = true;
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
