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
    pane split_direction="vertical" {{
        pane size=26 command="{e}" name="repositories" {{
            args "--view" "sidebar" "--session" "{t}" "{r}"
            close_on_exit true
        }}
        pane size=36 command="{e}" name="changes" {{
            args "--view" "changes" "--session" "{t}" "{r}"
            close_on_exit true
        }}
        pane command="{e}" name="graph | diff" {{
            args "--view" "main" "--session" "{t}" "{r}"
            close_on_exit true
        }}
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
        let kdl = layout_kdl("/usr/bin/twig-tui", "/home/u/repo", "p123");
        assert_eq!(
            kdl,
            r#"layout {
    pane split_direction="vertical" {
        pane size=26 command="/usr/bin/twig-tui" name="repositories" {
            args "--view" "sidebar" "--session" "p123" "/home/u/repo"
            close_on_exit true
        }
        pane size=36 command="/usr/bin/twig-tui" name="changes" {
            args "--view" "changes" "--session" "p123" "/home/u/repo"
            close_on_exit true
        }
        pane command="/usr/bin/twig-tui" name="graph | diff" {
            args "--view" "main" "--session" "p123" "/home/u/repo"
            close_on_exit true
        }
    }
}
"#
        );
    }

    #[test]
    fn layout_kdl_escapes_quotes_and_backslashes() {
        let kdl = layout_kdl("twig-tui", r#"/tmp/we"ird\path"#, "t");
        assert!(kdl.contains(r#""/tmp/we\"ird\\path""#));
    }
}
