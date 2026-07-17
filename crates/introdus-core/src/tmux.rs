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

/// The tmux user option each introdus session is tagged with: its canonical
/// project directory. Lets a later `introdus launch` from the same directory
/// find and reattach to the running session regardless of the session's
/// (name-hashed) name.
const PROJECT_OPTION: &str = "@introdus_project_dir";

/// Tag `session` with its canonical project directory (`dir`) so future
/// launches from the same directory can find it via [`find_session_by_project`].
pub fn set_session_project(session: &str, dir: &Path) -> Result<()> {
    tmux()
        .args(["set-option", "-t", session, PROJECT_OPTION])
        .arg(dir)
        .run()
}

/// The name of a live introdus session tagged with `dir` (its canonical project
/// directory), or `None`. Reads each session's [`PROJECT_OPTION`] with
/// `show-options -v` and returns the first exact match. Silent when no tmux
/// server is running.
///
/// Each option is read with a separate `show-options` call rather than a single
/// tab-delimited `list-sessions -F` line: some tmux builds (seen on Fedora's
/// 3.7b) mangle a literal tab embedded in the `-F` format, which would break a
/// delimiter-based parse. `show-options -v` returns the bare value, so there is
/// nothing to delimit.
pub fn find_session_by_project(dir: &Path) -> Option<String> {
    let target = dir.to_string_lossy();
    let names = tmux()
        .args(["list-sessions", "-F", "#{session_name}"])
        .stdout_quiet()
        .ok()?;
    names
        .lines()
        .find(|name| session_project(name).as_deref() == Some(&*target))
        .map(str::to_owned)
}

/// The value of [`PROJECT_OPTION`] on `session`, or `None` when the option is
/// unset (empty) or the session is gone.
fn session_project(session: &str) -> Option<String> {
    let value = tmux()
        .args(["show-options", "-t", session, "-v", PROJECT_OPTION])
        .stdout_quiet()
        .ok()?;
    (!value.is_empty()).then_some(value)
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
