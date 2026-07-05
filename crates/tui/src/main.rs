mod app;
mod keys;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use ratatui::crossterm::event::{self, Event};
use twig_core::watch::WorktreeWatcher;

use app::TuiApp;
use keys::KeyQueue;

enum Msg {
    Input(Event),
    Refresh,
}

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

    let (tx, rx) = mpsc::channel::<Msg>();

    let input_tx = tx.clone();
    thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if input_tx.send(Msg::Input(ev)).is_err() {
                break;
            }
        }
    });

    let watch_tx = tx;
    let watcher = WorktreeWatcher::new(
        &path,
        Arc::new(move || {
            let _ = watch_tx.send(Msg::Refresh);
        }),
    );
    if let Err(e) = &watcher {
        app.error = Some(format!("watcher: {e}"));
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &rx, watcher.ok());
    ratatui::restore();
    if let Err(e) = result {
        eprintln!("twig-tui: {e}");
        std::process::exit(1);
    }
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut TuiApp,
    rx: &mpsc::Receiver<Msg>,
    watcher: Option<WorktreeWatcher>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        let first = rx.recv()?;
        let mut refresh = false;
        let mut keys = Vec::new();
        for msg in std::iter::once(first).chain(rx.try_iter()) {
            match msg {
                Msg::Input(Event::Key(k)) => {
                    if let Some(nk) = keys::normalize(&k) {
                        keys.push(nk);
                    }
                }
                Msg::Input(_) => {}
                Msg::Refresh => refresh = true,
            }
        }

        if refresh && watcher.as_ref().is_none_or(|w| w.take_dirty()) {
            app.refresh();
        }
        app.handle_keys(KeyQueue(keys));
        if app.quit {
            return Ok(());
        }
    }
}
