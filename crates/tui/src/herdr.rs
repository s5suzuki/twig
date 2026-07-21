use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::session;

const SIDEBAR_COLS: f64 = 26.0;
const CHANGES_COLS: f64 = 36.0;
const TERM_MAIN_RATIO: f64 = 0.7;

pub fn inside_herdr() -> bool {
    std::env::var("HERDR_ENV").ok().as_deref() == Some("1")
}

pub fn current_pane_id() -> Option<String> {
    std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|v| !v.is_empty())
}

fn herdr(args: &[&str]) -> Result<Value, String> {
    let out = Command::new("herdr")
        .args(args)
        .output()
        .map_err(|e| format!("herdr not runnable: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .map_err(|_| format!("herdr {}: unparseable output", args.first().unwrap_or(&"")))?;
    if let Some(err) = value.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!("herdr {}: {msg}", args.join(" ")));
    }
    Ok(value)
}

fn pane_width(pane: &str) -> Option<f64> {
    let value = herdr(&["pane", "layout", "--pane", pane]).ok()?;
    value["result"]["layout"]["panes"]
        .as_array()?
        .iter()
        .find(|p| p["pane_id"].as_str() == Some(pane))
        .and_then(|p| p["rect"]["width"].as_f64())
}

fn split(pane: &str, direction: &str, ratio: f64, cwd: &Path) -> Result<String, String> {
    let ratio = ratio.clamp(0.05, 0.95).to_string();
    let value = herdr(&[
        "pane",
        "split",
        pane,
        "--direction",
        direction,
        "--ratio",
        &ratio,
        "--cwd",
        &cwd.to_string_lossy(),
        "--no-focus",
    ])?;
    value["result"]["pane"]["pane_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "herdr pane split: missing pane_id".to_string())
}

fn rename(pane: &str, label: &str) {
    let _ = herdr(&["pane", "rename", pane, label]);
}

fn run_exec(pane: &str, exe: &str, args: &[&str], repo: &Path) {
    let mut cmd = format!("exec {}", shell_quote(exe));
    for a in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(a));
    }
    cmd.push(' ');
    cmd.push_str(&shell_quote(&repo.to_string_lossy()));
    let _ = herdr(&["pane", "run", pane, &cmd]);
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn focus_pane(target: &str) {
    let mut last = String::new();
    for _ in 0..8 {
        let Ok(value) = herdr(&["pane", "layout", "--pane", target]) else {
            return;
        };
        let layout = &value["result"]["layout"];
        let Some(focused) = layout["focused_pane_id"].as_str() else {
            return;
        };
        if focused == target || focused == last {
            return;
        }
        last = focused.to_string();
        let center = |id: &str| -> Option<(f64, f64)> {
            layout["panes"].as_array()?.iter().find_map(|p| {
                if p["pane_id"].as_str() != Some(id) {
                    return None;
                }
                let r = &p["rect"];
                Some((
                    r["x"].as_f64()? + r["width"].as_f64()? / 2.0,
                    r["y"].as_f64()? + r["height"].as_f64()? / 2.0,
                ))
            })
        };
        let (Some((fx, fy)), Some((tx, ty))) = (center(focused), center(target)) else {
            return;
        };
        let (dx, dy) = (tx - fx, ty - fy);
        let dir = if dx.abs() >= dy.abs() {
            if dx >= 0.0 { "right" } else { "left" }
        } else if dy >= 0.0 {
            "down"
        } else {
            "up"
        };
        let _ = herdr(&["pane", "focus", "--direction", dir, "--current"]);
    }
}

pub fn close_pane(id: &str) {
    let _ = herdr(&["pane", "close", id]);
}

fn build_panes(exe: &str, sidebar: &str, repo: &Path, token: &str) -> Result<(), String> {
    let width = pane_width(sidebar).unwrap_or(120.0).max(SIDEBAR_COLS + 2.0);
    let changes = split(sidebar, "right", SIDEBAR_COLS / width, repo)?;

    let rest = pane_width(&changes).unwrap_or(width - SIDEBAR_COLS);
    let main = split(
        &changes,
        "right",
        CHANGES_COLS / rest.max(CHANGES_COLS + 2.0),
        repo,
    )?;

    let term = split(&main, "down", TERM_MAIN_RATIO, repo)?;

    rename(&changes, "changes");
    rename(&main, "graph | diff");
    rename(&term, "terminal");

    run_exec(
        &changes,
        exe,
        &["--view", "changes", "--session", token],
        repo,
    );
    run_exec(&main, exe, &["--view", "main", "--session", token], repo);
    run_exec(&term, exe, &["--shell", "--session", token], repo);
    Ok(())
}

pub fn split_current_tab(repo: &Path, token: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().into_owned();
    let sidebar = current_pane_id().ok_or("HERDR_PANE_ID not set")?;
    rename(&sidebar, "repositories");
    build_panes(&exe, &sidebar, repo, token)
}

pub fn spawn_tab(repo: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().into_owned();
    let token = session::pid_token();
    let dir = session::session_dir(&token);
    std::fs::create_dir_all(&dir).map_err(|e| format!("session dir: {e}"))?;

    let created = herdr(&[
        "tab",
        "create",
        "--cwd",
        &repo.to_string_lossy(),
        "--label",
        "twig",
        "--focus",
    ])?;
    let sidebar = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .ok_or("herdr tab create: missing root pane")?
        .to_string();
    rename(&sidebar, "repositories");
    build_panes(&exe, &sidebar, repo, &token)?;
    run_exec(
        &sidebar,
        &exe,
        &["--view", "sidebar", "--session", &token],
        repo,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_wraps_and_escapes() {
        assert_eq!(shell_quote("/tmp/repo"), "'/tmp/repo'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
