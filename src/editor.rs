use std::path::Path;
use std::process::Command;

pub fn open(repo_path: &Path, file: &str) -> Result<(), String> {
    let abs = repo_path.join(file);
    let abs = abs.to_string_lossy().into_owned();

    match nvim_server() {
        Some(server) => Command::new("nvim")
            .args(["--server", &server, "--remote", &abs])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to launch nvim --server: {e}")),
        None => open_in_terminal(&abs),
    }
}

pub fn open_in_server(repo_path: &Path, file: &str, server: &Path) -> Result<(), String> {
    open_abs_in_server(&repo_path.join(file), server)
}

pub fn open_abs_in_server(abs: &Path, server: &Path) -> Result<(), String> {
    Command::new("nvim")
        .args([
            "--server",
            &server.to_string_lossy(),
            "--remote",
            &abs.to_string_lossy(),
        ])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to send nvim --remote: {e}"))
}

fn nvim_server() -> Option<String> {
    ["TWIG_NVIM_ADDRESS", "NVIM"]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
}

fn open_in_terminal(abs: &str) -> Result<(), String> {
    let mut candidates: Vec<(String, &str)> = Vec::new();
    if let Ok(t) = std::env::var("TERMINAL") {
        candidates.push((t, "-e"));
    }
    for t in ["kitty", "alacritty", "wezterm", "foot", "xterm"] {
        candidates.push((t.to_string(), "-e"));
    }
    for t in ["gnome-terminal", "konsole"] {
        candidates.push((t.to_string(), "--"));
    }

    for (term, sep) in &candidates {
        let ok = Command::new(term).args([sep, "nvim", abs]).spawn().is_ok();
        if ok {
            return Ok(());
        }
    }
    Err(
        "No Neovim server ($NVIM/$TWIG_NVIM_ADDRESS) or terminal found. \
         Start nvim with `--listen <socket>` and set $TWIG_NVIM_ADDRESS to that path."
            .to_string(),
    )
}
