use std::path::Path;

use crate::session;

pub fn inside_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some()
}

pub fn layout_kdl(exe: &str, repo: &str, token: &str) -> String {
    let e = kdl_escape(exe);
    let r = kdl_escape(repo);
    let t = kdl_escape(token);
    format!(
        r#"layout {{
    pane size=1 borderless=true {{
        plugin location="zellij:tab-bar"
    }}
    pane split_direction="vertical" {{
        pane size=26 command="{e}" name="repositories" {{
            args "--view" "sidebar" "--session" "{t}" "{r}"
            close_on_exit true
        }}
        pane size=36 command="{e}" name="changes" {{
            args "--view" "changes" "--session" "{t}" "{r}"
            close_on_exit true
        }}
        pane split_direction="horizontal" {{
            pane command="{e}" name="graph | diff" {{
                args "--view" "main" "--session" "{t}" "{r}"
                close_on_exit true
            }}
            pane size="30%" command="{e}" name="terminal" {{
                args "--shell" "--session" "{t}" "{r}"
                close_on_exit true
            }}
        }}
    }}
    pane size=2 borderless=true {{
        plugin location="zellij:status-bar"
    }}
}}
"#
    )
}

fn kdl_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn action(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("zellij")
        .arg("action")
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn focused_pane() -> Option<String> {
    let out = action(&["list-clients"])?;
    let id = out.lines().nth(1)?.split_whitespace().nth(1)?;
    Some(id.trim_start_matches("terminal_").to_string())
}

pub fn focus_pane(target: &str) {
    let _ = action(&["focus-pane-id", target]);
    for _ in 0..4 {
        let cur = focused_pane();
        if cur.is_none() || cur.as_deref() == Some(target) {
            return;
        }
        let _ = action(&["move-focus", "right"]);
        if focused_pane() == cur {
            return;
        }
    }
}

pub fn split_current_tab(repo: &Path, token: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    run_pane(
        &exe,
        repo,
        "changes",
        "right",
        &["--view", "changes", "--session", token, "--cols", "36"],
    )?;
    run_pane(
        &exe,
        repo,
        "graph | diff",
        "right",
        &["--view", "main", "--session", token],
    )?;
    run_pane(
        &exe,
        repo,
        "terminal",
        "down",
        &["--shell", "--session", token, "--shrink", "4"],
    )?;
    let _ = action(&["move-focus", "up"]);
    let _ = action(&["move-focus", "left"]);
    Ok(())
}

fn run_pane(
    exe: &Path,
    repo: &Path,
    name: &str,
    direction: &str,
    args: &[&str],
) -> Result<String, String> {
    let mut cmd = std::process::Command::new("zellij");
    cmd.args(["run", "-d", direction, "-c", "-n", name, "--"])
        .arg(exe)
        .args(args)
        .arg(repo);
    let out = cmd
        .output()
        .map_err(|e| format!("zellij not runnable: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "zellij run failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn close_pane(id: &str) {
    let _ = action(&["close-pane", "--pane-id", id]);
}

pub fn resize_self_step(direction: &str) {
    if let Ok(id) = std::env::var("ZELLIJ_PANE_ID")
        && !id.is_empty()
    {
        let _ = action(&["resize", "decrease", direction, "--pane-id", &id]);
    }
}

pub fn spawn_tab(repo: &Path) -> Result<(), String> {
    let token = session::pid_token();
    let dir = session::session_dir(&token);
    std::fs::create_dir_all(&dir).map_err(|e| format!("session dir: {e}"))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let kdl = layout_kdl(&exe.to_string_lossy(), &repo.to_string_lossy(), &token);
    let layout_path = dir.join("layout.kdl");
    std::fs::write(&layout_path, kdl).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("zellij")
        .args(["action", "new-tab", "--layout"])
        .arg(&layout_path)
        .status()
        .map_err(|e| format!("zellij not runnable: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("zellij exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_kdl_snapshot() {
        let kdl = layout_kdl("/usr/bin/twit", "/home/u/repo", "p123");
        assert_eq!(
            kdl,
            r#"layout {
    pane size=1 borderless=true {
        plugin location="zellij:tab-bar"
    }
    pane split_direction="vertical" {
        pane size=26 command="/usr/bin/twit" name="repositories" {
            args "--view" "sidebar" "--session" "p123" "/home/u/repo"
            close_on_exit true
        }
        pane size=36 command="/usr/bin/twit" name="changes" {
            args "--view" "changes" "--session" "p123" "/home/u/repo"
            close_on_exit true
        }
        pane split_direction="horizontal" {
            pane command="/usr/bin/twit" name="graph | diff" {
                args "--view" "main" "--session" "p123" "/home/u/repo"
                close_on_exit true
            }
            pane size="30%" command="/usr/bin/twit" name="terminal" {
                args "--shell" "--session" "p123" "/home/u/repo"
                close_on_exit true
            }
        }
    }
    pane size=2 borderless=true {
        plugin location="zellij:status-bar"
    }
}
"#
        );
    }

    #[test]
    fn layout_kdl_escapes_quotes_and_backslashes() {
        let kdl = layout_kdl("twit", r#"/tmp/we"ird\path"#, "t");
        assert!(kdl.contains(r#""/tmp/we\"ird\\path""#));
    }
}
