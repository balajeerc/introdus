//! Thin `tmux` helpers built on [`crate::process::Cmd`].
//!
//! The control plane lives inside one tmux session per container: a
//! `main-control` window (the TUI), a `dev-container` window (podman logs), and
//! on-demand `root-bash` / `dev-bash` / per-agent windows.

use std::path::Path;

use crate::process::Cmd;
use anyhow::Result;

/// A fresh `tmux` invocation.
pub fn tmux() -> Cmd {
    Cmd::new("tmux")
}

/// True if a session with this name exists.
pub fn has_session(name: &str) -> bool {
    tmux().args(["has-session", "-t", name]).ok()
}

/// Create a detached session whose first window `window` runs `command`, with
/// `cwd` as the window's start directory.
pub fn new_detached_session(session: &str, window: &str, command: &str, cwd: &Path) -> Result<()> {
    tmux()
        .args(["new-session", "-d", "-s", session, "-n", window, "-c"])
        .arg(cwd)
        .arg(command)
        .run()
}

/// Add a window named `window` running `command` (start dir `cwd`) to an
/// existing session. `select` brings the new window to the foreground.
pub fn new_window(
    session: &str,
    window: &str,
    command: &str,
    select: bool,
    cwd: &Path,
) -> Result<()> {
    let mut c = tmux()
        .args(["new-window", "-t", session, "-n", window, "-c"])
        .arg(cwd);
    if !select {
        c = c.arg("-d");
    }
    c.arg(command).run()
}

/// Kill a specific window (`session:window`) if present; ignores absence.
pub fn kill_window(session: &str, window: &str) -> Result<()> {
    let target = format!("{session}:{window}");
    if tmux()
        .args(["list-windows", "-t", session, "-F", "#{window_name}"])
        .ok()
    {
        let _ = tmux().args(["kill-window", "-t", &target]).run();
    }
    Ok(())
}

/// Kill a session if it exists.
pub fn kill_session(session: &str) -> Result<()> {
    if has_session(session) {
        tmux().args(["kill-session", "-t", session]).run()?;
    }
    Ok(())
}

/// An `exec`-able invocation that attaches the caller's terminal to `session`.
pub fn attach(session: &str) -> Cmd {
    tmux().args(["attach-session", "-t", session])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta27_attach_label() {
        assert_eq!(
            attach("introdus-web-ab12").label(),
            "tmux attach-session -t introdus-web-ab12"
        );
    }
}
